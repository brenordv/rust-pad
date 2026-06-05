/// Per-file view-state persistence: stores the scroll position and
/// cursor coordinates of every file the user has opened so they can be
/// restored on re-open.
///
/// The store is keyed by a canonical path string (see
/// [`crate::paths::canonical_path_key`]). Canonicalization is performed
/// on **save**, never on read — this avoids a TOCTOU window where a
/// swapped file could cause restored state to land on the wrong document.
///
/// Persistence schema is intentionally separate from [`crate::session`]
/// because bincode is a positional format: adding fields to
/// [`crate::session::SessionTabEntry`] would break compatibility with
/// existing session files. A sibling store is zero-risk.
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use redb::{Database, ReadableDatabase, ReadableTable, ReadableTableMetadata, TableDefinition};
use serde::{Deserialize, Serialize};

use crate::db_helpers::{deserialize_record, open_or_create_db, read_table, write_table};

/// Maximum size in bytes for deserializing a `ViewState` value.
/// 1 MB is generous; the realistic record size is < 100 bytes.
const MAX_VIEW_STATE_BYTES: u64 = 1024 * 1024;

/// Soft cap on the number of entries kept in the store.
///
/// Once exceeded, the oldest entries (by `last_used_unix_ms`) are pruned
/// on the next save so the table cannot grow without bound across
/// multi-year usage.
const VIEW_STATE_ENTRY_CAP: usize = 1000;

/// View-state table: canonical-path → bincode-encoded `ViewState`.
const VIEW_STATE_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("view_state");

/// Per-file view-state: scroll position and cursor coordinates plus a
/// last-used timestamp used for LRU prune.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct ViewState {
    /// Vertical scroll offset (line index of the top visible line).
    pub scroll_y: f32,
    /// Horizontal scroll offset in pixels.
    pub scroll_x: f32,
    /// Cursor line index.
    pub cursor_line: usize,
    /// Cursor column (char index within the line).
    pub cursor_col: usize,
    /// Wall-clock timestamp of the last `save` for LRU pruning. Milliseconds
    /// since the Unix epoch. A value of 0 (default) marks entries from
    /// pre-versioned writes; they are pruned first.
    pub last_used_unix_ms: i64,
}

impl Default for ViewState {
    fn default() -> Self {
        Self {
            scroll_y: 0.0,
            scroll_x: 0.0,
            cursor_line: 0,
            cursor_col: 0,
            last_used_unix_ms: 0,
        }
    }
}

/// Persistence layer for view-state, backed by redb.
pub struct ViewStateStore {
    db: Database,
}

impl std::fmt::Debug for ViewStateStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ViewStateStore").finish()
    }
}

impl ViewStateStore {
    /// Returns the default view-state database path in the platform-standard
    /// data directory.
    pub fn view_state_path() -> PathBuf {
        crate::paths::view_state_file_path()
    }

    /// Opens or creates the view-state database at `path`.
    ///
    /// Creates the parent directory if it does not exist.
    ///
    /// # Errors
    ///
    /// Returns an error if the redb file cannot be opened or the schema
    /// table cannot be created.
    pub fn open(path: &Path) -> Result<Self> {
        let db = open_or_create_db(path, "view_state")?;

        // Ensure the table exists.
        let write_txn = db
            .begin_write()
            .context("Failed to begin initial view-state write transaction")?;
        {
            let _ = write_txn
                .open_table(VIEW_STATE_TABLE)
                .context("Failed to create view_state table")?;
        }
        write_txn
            .commit()
            .context("Failed to commit initial view-state transaction")?;

        Ok(Self { db })
    }

    /// Loads the `ViewState` for `key`, or `None` if no entry exists.
    ///
    /// Corrupt records are silently discarded — same convention as
    /// [`crate::workspace::WorkspaceStore`] and [`crate::session::SessionStore`].
    ///
    /// # Errors
    ///
    /// Returns an error only if the redb transaction itself fails.
    pub fn load(&self, key: &str) -> Result<Option<ViewState>> {
        read_table!(self.db, VIEW_STATE_TABLE, |table| {
            match table.get(key).context("Failed to read view-state entry")? {
                Some(guard) => {
                    let decoded = deserialize_record::<ViewState>(
                        guard.value(),
                        MAX_VIEW_STATE_BYTES,
                        "view-state entry",
                    );
                    if decoded.is_none() {
                        tracing::debug!("Discarded corrupt view-state entry for key '{key}'");
                    }
                    Ok(decoded)
                }
                None => Ok(None),
            }
        })
    }

    /// Saves `state` under `key`. Prunes the oldest entries when the
    /// total count exceeds [`VIEW_STATE_ENTRY_CAP`].
    ///
    /// # Errors
    ///
    /// Returns an error if serialization or the redb write fails.
    pub fn save(&self, key: &str, state: &ViewState) -> Result<()> {
        let bytes = bincode::serialize(state).context("Failed to serialize view-state")?;

        write_table!(self.db, VIEW_STATE_TABLE, |table| {
            table
                .insert(key, bytes.as_slice())
                .context("Failed to insert view-state entry")?;
            anyhow::Ok(())
        })?;

        self.prune_if_over_cap()?;
        Ok(())
    }

    /// If the entry count exceeds [`VIEW_STATE_ENTRY_CAP`], deletes the
    /// 10% oldest entries by `last_used_unix_ms`.
    fn prune_if_over_cap(&self) -> Result<()> {
        let count = self.entry_count()?;
        if count <= VIEW_STATE_ENTRY_CAP {
            return Ok(());
        }

        let mut entries = self.list_entries()?;
        entries.sort_by_key(|(_, state)| state.last_used_unix_ms);
        let drop_count = (count / 10).max(1);
        let to_remove: Vec<String> = entries
            .into_iter()
            .take(drop_count)
            .map(|(k, _)| k)
            .collect();

        write_table!(self.db, VIEW_STATE_TABLE, |table| {
            for key in &to_remove {
                let _ = table.remove(key.as_str());
            }
            anyhow::Ok(())
        })?;

        tracing::debug!(
            "ViewStateStore pruned {removed} stale entries (count was {count})",
            removed = to_remove.len(),
        );
        Ok(())
    }

    /// Returns the number of entries currently in the table.
    fn entry_count(&self) -> Result<usize> {
        read_table!(self.db, VIEW_STATE_TABLE, |table| {
            let n = table.len().context("Failed to count view-state entries")?;
            Ok(usize::try_from(n).unwrap_or(usize::MAX))
        })
    }

    /// Reads every `(key, ViewState)` pair currently in the table.
    /// Used internally for prune; not exposed publicly because callers
    /// should look up by key rather than scan.
    fn list_entries(&self) -> Result<Vec<(String, ViewState)>> {
        read_table!(self.db, VIEW_STATE_TABLE, |table| {
            let mut out = Vec::new();
            for entry in table.iter().context("Failed to iterate view-state table")? {
                let (k, v) = match entry {
                    Ok(pair) => pair,
                    Err(e) => {
                        tracing::warn!("Skipping unreadable view-state entry: {e}");
                        continue;
                    }
                };
                let key = k.value().to_string();
                if let Some(state) = deserialize_record::<ViewState>(
                    v.value(),
                    MAX_VIEW_STATE_BYTES,
                    "view-state entry",
                ) {
                    out.push((key, state));
                }
            }
            Ok(out)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn open_test_store() -> (ViewStateStore, TempDir) {
        let dir = TempDir::new().expect("create temp dir");
        let db_path = dir.path().join("test-view-state.redb");
        let store = ViewStateStore::open(&db_path).expect("open view-state store");
        (store, dir)
    }

    fn vs(scroll_y: f32, line: usize, col: usize) -> ViewState {
        ViewState {
            scroll_y,
            scroll_x: 0.0,
            cursor_line: line,
            cursor_col: col,
            last_used_unix_ms: 1,
        }
    }

    #[test]
    fn empty_store_returns_none() {
        let (store, _dir) = open_test_store();
        let result = store.load("/nonexistent").expect("load");
        assert!(result.is_none());
    }

    #[test]
    fn save_and_load_roundtrip() {
        let (store, _dir) = open_test_store();
        let original = vs(42.5, 100, 7);
        store.save("/tmp/foo.rs", &original).expect("save");

        let loaded = store.load("/tmp/foo.rs").expect("load").expect("some");
        assert_eq!(loaded, original);
    }

    #[test]
    fn upsert_replaces_existing_entry() {
        let (store, _dir) = open_test_store();
        store.save("/key", &vs(1.0, 1, 1)).expect("save 1");
        store.save("/key", &vs(2.0, 2, 2)).expect("save 2");
        let loaded = store.load("/key").expect("load").expect("some");
        assert_eq!(loaded, vs(2.0, 2, 2));
    }

    #[test]
    fn corrupted_entry_returns_none() {
        let (store, _dir) = open_test_store();

        let write_txn = store.db.begin_write().expect("txn");
        {
            let mut table = write_txn.open_table(VIEW_STATE_TABLE).expect("table");
            table
                .insert("/corrupt", &[0xFFu8, 0xFF, 0xFF][..])
                .expect("insert");
        }
        write_txn.commit().expect("commit");

        let result = store.load("/corrupt").expect("should not error");
        assert!(result.is_none());
    }

    #[test]
    fn default_view_state_is_zeroed() {
        let v = ViewState::default();
        assert_eq!(v.scroll_y, 0.0);
        assert_eq!(v.scroll_x, 0.0);
        assert_eq!(v.cursor_line, 0);
        assert_eq!(v.cursor_col, 0);
    }

    #[test]
    fn view_state_path_is_non_empty() {
        let path = ViewStateStore::view_state_path();
        assert!(!path.as_os_str().is_empty());
    }

    #[test]
    fn prune_removes_oldest_when_over_cap() {
        let (store, _dir) = open_test_store();
        // Insert one more than the cap with monotonically increasing
        // timestamps so prune order is deterministic.
        for i in 0..(VIEW_STATE_ENTRY_CAP + 5) {
            let s = ViewState {
                scroll_y: 0.0,
                scroll_x: 0.0,
                cursor_line: 0,
                cursor_col: 0,
                last_used_unix_ms: i as i64,
            };
            store.save(&format!("/file/{i}"), &s).expect("save");
        }

        // The very oldest entry should have been pruned.
        let oldest = store.load("/file/0").expect("load");
        assert!(
            oldest.is_none(),
            "Oldest entry should be pruned when over cap"
        );

        // Newer entries should remain.
        let newest = store
            .load(&format!("/file/{}", VIEW_STATE_ENTRY_CAP + 4))
            .expect("load");
        assert!(newest.is_some());
    }

    #[test]
    fn view_state_serde_roundtrip() {
        let v = vs(123.456, 999, 12);
        let bytes = bincode::serialize(&v).expect("serialize");
        let decoded: ViewState = bincode::deserialize(&bytes).expect("deserialize");
        assert_eq!(decoded, v);
    }

    #[test]
    fn view_state_store_debug() {
        let (store, _dir) = open_test_store();
        let debug = format!("{store:?}");
        assert!(debug.contains("ViewStateStore"));
    }
}
