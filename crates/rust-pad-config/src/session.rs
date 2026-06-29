/// Session persistence: saves/restores open tabs across app restarts.
///
/// Metadata (tab order, active index) is stored in a redb table as bincode.
/// Content of unsaved tabs is stored as raw `&str` in a separate table,
/// avoiding JSON escaping issues with special characters or large buffers.
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{Context, Result};
use redb::{Database, ReadableDatabase, ReadableTable, TableDefinition};
use serde::{Deserialize, Serialize};

use crate::db_helpers::{deserialize_record, open_or_create_db, read_table, write_table};

/// Maximum size in bytes for deserializing session metadata.
/// 10 MB is generous for tab metadata; anything larger is likely corrupt.
const MAX_SESSION_META_BYTES: u64 = 10 * 1024 * 1024;

/// Hard ceiling on a single stored content row read from disk. The content
/// table is recovery-critical and read on every cold start from a
/// user-writable file, so a corrupt/tampered row must not be able to force an
/// unbounded allocation. 100 MB mirrors the config-side content clamp ceiling.
const MAX_SESSION_CONTENT_BYTES: usize = 100 * 1024 * 1024;

/// Sibling key in [`SESSION_META`] holding the clean-shutdown flag (`[1]` =
/// clean, `[0]` = an autosave snapshot that was not yet followed by a clean
/// exit). Stored as a separate key rather than a [`SessionData`] field so it
/// does not break the bincode format of existing sessions.
const CLEAN_SHUTDOWN_KEY: &str = "clean_shutdown";

/// Session metadata table: `"data"` → bincode(`SessionData`).
const SESSION_META: TableDefinition<&str, &[u8]> = TableDefinition::new("session_meta");

/// Unsaved tab content table: session_id → raw text.
const SESSION_CONTENT: TableDefinition<&str, &str> = TableDefinition::new("session_content");

/// Counter for generating unique session IDs within a process lifetime.
static SESSION_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Generates a unique session ID for an unsaved tab.
pub fn generate_session_id() -> String {
    let n = SESSION_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("sess-{n}")
}

/// Describes one tab in the session.
///
/// Serialization format note: this enum is persisted via **bincode**, which
/// is a positional binary format with no per-field schema. Adding fields to
/// existing variants is therefore a breaking change — old session files will
/// fail to deserialize and the existing corruption handler in
/// [`SessionStore::load_session`] will discard them, starting fresh. This
/// trade-off is intentional; documented in `CHANGELOG.md` for v2.0.0.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SessionTabEntry {
    /// A file-backed tab (just needs its path to reopen).
    File {
        path: String,
        /// Whether this tab is pinned.
        pinned: bool,
        /// Optional tab color, stored as a stable string identifier (see
        /// `rust_pad_core::tab_color::TabColor::as_serde_str`). Stored as a
        /// string rather than the enum so future palette additions don't
        /// require an enum-tag bump.
        tab_color: Option<String>,
    },
    /// An unsaved/untitled tab whose content is stored in the session DB.
    Unsaved {
        session_id: String,
        title: String,
        pinned: bool,
        tab_color: Option<String>,
    },
}

/// Persisted split-view state. `None` (top-level) means the previous
/// session was in single-pane mode.
///
/// Tab indices are positions inside [`SessionData::tabs`], not document
/// indices in the running app. They are translated back to live document
/// indices on restore by [`crate::session`] consumers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSplit {
    /// `"vertical"` or `"horizontal"`. Stored as a string so future
    /// orientation additions don't shift bincode tags.
    pub orientation: String,
    /// Fraction of the central panel allocated to the Left/top pane.
    pub divider_ratio: f32,
    /// Indices into `SessionData::tabs` belonging to the Left pane.
    pub left_tab_indices: Vec<usize>,
    /// Indices into `SessionData::tabs` belonging to the Right pane.
    pub right_tab_indices: Vec<usize>,
    /// Index into `left_tab_indices` for the Left pane's active tab.
    pub left_active: usize,
    /// Index into `right_tab_indices` for the Right pane's active tab.
    pub right_active: usize,
    /// `"left"` or `"right"`.
    pub focused: String,
}

/// The full session state: ordered list of tabs + which one was active +
/// optional split view layout.
///
/// **Versioning note:** see [`SessionTabEntry`] — adding fields is a
/// breaking change for the bincode format. Old session files that
/// predate this struct's `split` field will fail to deserialize and be
/// discarded by [`SessionStore::load_session`]'s corruption handler;
/// the user will see a fresh empty workspace once.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionData {
    pub tabs: Vec<SessionTabEntry>,
    pub active_tab_index: usize,
    /// Split-view layout, or `None` for single-pane sessions.
    pub split: Option<SessionSplit>,
}

/// Persistence layer for session state, backed by redb.
pub struct SessionStore {
    db: Database,
}

impl std::fmt::Debug for SessionStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SessionStore").finish()
    }
}

impl SessionStore {
    /// Returns the default session database path in the platform-standard
    /// data directory.
    ///
    /// Falls back to the executable directory if the platform data
    /// directory cannot be determined.
    pub fn session_path() -> PathBuf {
        crate::paths::session_file_path()
    }

    /// Opens or creates the session database at `path`.
    ///
    /// Creates the parent directory if it does not exist.
    pub fn open(path: &Path) -> Result<Self> {
        let db = open_or_create_db(path, "session")?;

        // Ensure tables exist
        let write_txn = db
            .begin_write()
            .context("Failed to begin initial session write transaction")?;
        {
            let _ = write_txn
                .open_table(SESSION_META)
                .context("Failed to create session_meta table")?;
            let _ = write_txn
                .open_table(SESSION_CONTENT)
                .context("Failed to create session_content table")?;
        }
        write_txn
            .commit()
            .context("Failed to commit initial session transaction")?;

        Ok(Self { db })
    }

    /// Atomically persists the whole session in a single write transaction:
    /// the content table is cleared and rewritten from `content`, and the
    /// metadata + clean-shutdown flag are written, all committed together.
    ///
    /// Because redb is ACID per transaction, a crash mid-commit leaves the
    /// previous consistent snapshot intact — `session_meta` and
    /// `session_content` can never disagree on disk. This replaces the older
    /// `save_session` + per-tab `save_content` + `clear_all_content` trio.
    ///
    /// `clean_shutdown` records whether this is the final snapshot of a clean
    /// exit (`true`) or an in-flight autosave (`false`); the next launch reads
    /// it via [`SessionStore::was_clean_shutdown`] to detect crashes.
    ///
    /// The two tables are deliberately opened with a hand-rolled transaction
    /// rather than the single-table `write_table!` macro: atomicity across
    /// both tables is the whole point and the macro scopes one table per
    /// commit.
    pub fn save_snapshot(
        &self,
        meta: &SessionData,
        content: &[(String, String)],
        clean_shutdown: bool,
    ) -> Result<()> {
        let bytes = bincode::serialize(meta).context("Failed to serialize session data")?;

        let write_txn = self
            .db
            .begin_write()
            .context("Failed to begin session snapshot transaction")?;
        {
            // Rewrite the content table from scratch so closed/renamed tabs
            // and pre-restore orphans (under old session ids) leave no residue.
            let mut content_table = write_txn
                .open_table(SESSION_CONTENT)
                .context("Failed to open session_content table")?;
            let stale: Vec<String> = content_table
                .iter()
                .context("Failed to iterate session_content")?
                .filter_map(|entry| entry.ok().map(|(k, _)| k.value().to_string()))
                .collect();
            for key in &stale {
                content_table
                    .remove(key.as_str())
                    .context("Failed to clear stale session content")?;
            }
            for (session_id, text) in content {
                content_table
                    .insert(session_id.as_str(), text.as_str())
                    .context("Failed to insert session content")?;
            }

            let mut meta_table = write_txn
                .open_table(SESSION_META)
                .context("Failed to open session_meta table")?;
            meta_table
                .insert("data", bytes.as_slice())
                .context("Failed to insert session data")?;
            let flag = [u8::from(clean_shutdown)];
            meta_table
                .insert(CLEAN_SHUTDOWN_KEY, flag.as_slice())
                .context("Failed to insert clean-shutdown flag")?;
        }
        write_txn
            .commit()
            .context("Failed to commit session snapshot")?;
        Ok(())
    }

    /// Returns whether the previous run recorded a clean shutdown. An absent
    /// flag (no prior session, or one predating the flag) is treated as clean
    /// so first launches never report a false recovery.
    pub fn was_clean_shutdown(&self) -> Result<bool> {
        read_table!(self.db, SESSION_META, |table| {
            match table
                .get(CLEAN_SHUTDOWN_KEY)
                .context("Failed to read clean-shutdown flag")?
            {
                Some(guard) => Ok(guard.value().first().copied().unwrap_or(1) != 0),
                None => Ok(true),
            }
        })
    }

    /// Loads the session tab list, or `None` if no session was saved.
    pub fn load_session(&self) -> Result<Option<SessionData>> {
        read_table!(self.db, SESSION_META, |table| {
            let data = match table.get("data").context("Failed to read session data")? {
                Some(guard) => deserialize_record::<SessionData>(
                    guard.value(),
                    MAX_SESSION_META_BYTES,
                    "session data",
                ),
                None => None,
            };
            Ok(data)
        })
    }

    /// Loads the content of an unsaved tab, or `None` if not found.
    ///
    /// Enforces [`MAX_SESSION_CONTENT_BYTES`] on the stored value *before*
    /// materializing it, so a corrupt or tampered row cannot force an
    /// unbounded allocation on startup. An oversized row is skipped (treated
    /// as absent) with a count-only `tracing::warn!` — never echoing content.
    pub fn load_content(&self, session_id: &str) -> Result<Option<String>> {
        read_table!(self.db, SESSION_CONTENT, |table| {
            match table
                .get(session_id)
                .context("Failed to read session content")?
            {
                Some(guard) => {
                    let len = guard.value().len();
                    if len > MAX_SESSION_CONTENT_BYTES {
                        tracing::warn!(
                            len,
                            limit = MAX_SESSION_CONTENT_BYTES,
                            "Skipping oversized session content row (possible corruption)"
                        );
                        Ok(None)
                    } else {
                        Ok(Some(guard.value().to_string()))
                    }
                }
                None => Ok(None),
            }
        })
    }

    /// Deletes the content for one unsaved tab (cleanup on tab close).
    pub fn delete_content(&self, session_id: &str) -> Result<()> {
        write_table!(self.db, SESSION_CONTENT, |table| {
            let _ = table.remove(session_id);
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn open_test_store() -> (SessionStore, TempDir) {
        let dir = TempDir::new().expect("create temp dir");
        let db_path = dir.path().join("test-session.redb");
        let store = SessionStore::open(&db_path).expect("open session store");
        (store, dir)
    }

    #[test]
    fn test_load_empty_session() {
        let (store, _dir) = open_test_store();
        let result = store.load_session().expect("load");
        assert!(result.is_none());
    }

    #[test]
    fn test_save_and_load_session() {
        let (store, _dir) = open_test_store();

        let data = SessionData {
            tabs: vec![
                SessionTabEntry::File {
                    path: "/tmp/foo.rs".to_string(),
                    pinned: true,
                    tab_color: Some("blue".to_string()),
                },
                SessionTabEntry::Unsaved {
                    session_id: "sess-0".to_string(),
                    title: "Untitled".to_string(),
                    pinned: false,
                    tab_color: None,
                },
            ],
            active_tab_index: 1,
            split: None,
        };

        store.save_snapshot(&data, &[], true).expect("save");
        let loaded = store.load_session().expect("load").expect("some");

        assert_eq!(loaded.tabs.len(), 2);
        assert_eq!(loaded.active_tab_index, 1);

        match &loaded.tabs[0] {
            SessionTabEntry::File {
                path,
                pinned,
                tab_color,
            } => {
                assert_eq!(path, "/tmp/foo.rs");
                assert!(*pinned);
                assert_eq!(tab_color.as_deref(), Some("blue"));
            }
            _ => panic!("expected File entry"),
        }
        match &loaded.tabs[1] {
            SessionTabEntry::Unsaved {
                session_id,
                title,
                pinned,
                tab_color,
            } => {
                assert_eq!(session_id, "sess-0");
                assert_eq!(title, "Untitled");
                assert!(!*pinned);
                assert!(tab_color.is_none());
            }
            _ => panic!("expected Unsaved entry"),
        }
    }

    /// Builds a `SessionData` of `n` unsaved tabs named `sess-0..sess-n`.
    fn unsaved_session(ids: &[&str]) -> SessionData {
        SessionData {
            tabs: ids
                .iter()
                .map(|id| SessionTabEntry::Unsaved {
                    session_id: (*id).to_string(),
                    title: format!("Tab {id}"),
                    pinned: false,
                    tab_color: None,
                })
                .collect(),
            active_tab_index: 0,
            split: None,
        }
    }

    #[test]
    fn test_snapshot_persists_content() {
        let (store, _dir) = open_test_store();

        let meta = unsaved_session(&["sess-1"]);
        let content = vec![(
            "sess-1".to_string(),
            "Hello, world!\nLine 2\n\tTabbed".to_string(),
        )];
        store.save_snapshot(&meta, &content, false).expect("save");

        let loaded = store.load_content("sess-1").expect("load").expect("some");
        assert_eq!(loaded, "Hello, world!\nLine 2\n\tTabbed");
    }

    #[test]
    fn test_delete_content() {
        let (store, _dir) = open_test_store();

        let meta = unsaved_session(&["sess-2"]);
        store
            .save_snapshot(
                &meta,
                &[("sess-2".to_string(), "some text".to_string())],
                false,
            )
            .expect("save");
        assert!(store.load_content("sess-2").expect("load").is_some());

        store.delete_content("sess-2").expect("delete");
        assert!(store.load_content("sess-2").expect("load").is_none());
    }

    /// Regression for the reported data-loss bug: a snapshot must atomically
    /// drop content for session ids no longer present (closed tabs, or the
    /// old ids assigned before a restore) so `session_meta` and
    /// `session_content` never reference stale/missing rows.
    #[test]
    fn test_snapshot_drops_stale_content() {
        let (store, _dir) = open_test_store();

        let first = unsaved_session(&["a", "b", "c"]);
        let first_content = vec![
            ("a".to_string(), "content-a".to_string()),
            ("b".to_string(), "content-b".to_string()),
            ("c".to_string(), "content-c".to_string()),
        ];
        store
            .save_snapshot(&first, &first_content, false)
            .expect("first");

        // Second snapshot keeps only `b` (a/c "closed") under a fresh id `d`.
        let second = unsaved_session(&["b", "d"]);
        let second_content = vec![
            ("b".to_string(), "content-b2".to_string()),
            ("d".to_string(), "content-d".to_string()),
        ];
        store
            .save_snapshot(&second, &second_content, false)
            .expect("second");

        assert!(
            store.load_content("a").expect("load").is_none(),
            "a must be dropped"
        );
        assert!(
            store.load_content("c").expect("load").is_none(),
            "c must be dropped"
        );
        assert_eq!(
            store.load_content("b").expect("load").as_deref(),
            Some("content-b2")
        );
        assert_eq!(
            store.load_content("d").expect("load").as_deref(),
            Some("content-d")
        );
    }

    /// Every `Unsaved` meta entry must have matching content after a snapshot —
    /// the invariant whose violation caused the reported bug.
    #[test]
    fn test_snapshot_meta_and_content_consistent() {
        let (store, _dir) = open_test_store();

        let meta = unsaved_session(&["x", "y"]);
        let content = vec![
            ("x".to_string(), "x-text".to_string()),
            ("y".to_string(), "y-text".to_string()),
        ];
        store.save_snapshot(&meta, &content, false).expect("save");

        let loaded = store.load_session().expect("load").expect("some");
        for tab in &loaded.tabs {
            if let SessionTabEntry::Unsaved { session_id, .. } = tab {
                assert!(
                    store.load_content(session_id).expect("load").is_some(),
                    "every Unsaved entry must have content: {session_id}"
                );
            }
        }
    }

    #[test]
    fn test_clean_shutdown_flag_roundtrip() {
        let (store, _dir) = open_test_store();

        // Absent flag → treated as clean (no false recovery on first launch).
        assert!(store.was_clean_shutdown().expect("read"));

        let meta = unsaved_session(&["s"]);
        store.save_snapshot(&meta, &[], false).expect("autosave");
        assert!(
            !store.was_clean_shutdown().expect("read"),
            "autosave is unclean"
        );

        store.save_snapshot(&meta, &[], true).expect("clean exit");
        assert!(store.was_clean_shutdown().expect("read"), "clean exit");
    }

    #[test]
    fn test_session_tab_entry_serde() {
        let file_entry = SessionTabEntry::File {
            path: "test.txt".to_string(),
            pinned: true,
            tab_color: Some("red".to_string()),
        };
        let unsaved_entry = SessionTabEntry::Unsaved {
            session_id: "sess-42".to_string(),
            title: "My Tab".to_string(),
            pinned: false,
            tab_color: None,
        };

        // bincode round-trip
        let bytes1 = bincode::serialize(&file_entry).expect("serialize");
        let decoded1: SessionTabEntry = bincode::deserialize(&bytes1).expect("deserialize");
        match decoded1 {
            SessionTabEntry::File {
                path,
                pinned,
                tab_color,
            } => {
                assert_eq!(path, "test.txt");
                assert!(pinned);
                assert_eq!(tab_color.as_deref(), Some("red"));
            }
            _ => panic!("expected File"),
        }

        let bytes2 = bincode::serialize(&unsaved_entry).expect("serialize");
        let decoded2: SessionTabEntry = bincode::deserialize(&bytes2).expect("deserialize");
        match decoded2 {
            SessionTabEntry::Unsaved {
                session_id,
                title,
                pinned,
                tab_color,
            } => {
                assert_eq!(session_id, "sess-42");
                assert_eq!(title, "My Tab");
                assert!(!pinned);
                assert!(tab_color.is_none());
            }
            _ => panic!("expected Unsaved"),
        }
    }

    #[test]
    fn test_content_with_special_chars() {
        let (store, _dir) = open_test_store();

        // Test with unicode, quotes, backslashes, null-like patterns
        let content = "Hello 🌍\n\"quotes\" and \\backslash\n\t\ttabs\nline with \0 null";
        let meta = unsaved_session(&["special"]);
        store
            .save_snapshot(
                &meta,
                &[("special".to_string(), content.to_string())],
                false,
            )
            .expect("save");
        let loaded = store.load_content("special").expect("load").expect("some");
        assert_eq!(loaded, content);
    }

    #[test]
    fn test_generate_session_id_unique() {
        let id1 = generate_session_id();
        let id2 = generate_session_id();
        assert_ne!(id1, id2);
        assert!(id1.starts_with("sess-"));
        assert!(id2.starts_with("sess-"));
    }

    // ── Deserialization size limits ──────────────────────────────────

    #[test]
    fn test_load_session_returns_none_on_corrupt_data() {
        let (store, _dir) = open_test_store();

        // Write valid session data first
        let data = SessionData {
            tabs: vec![SessionTabEntry::File {
                path: "/tmp/foo.rs".to_string(),
                pinned: false,
                tab_color: None,
            }],
            active_tab_index: 0,
            split: None,
        };
        store.save_snapshot(&data, &[], true).expect("save");
        assert!(store.load_session().expect("load").is_some());

        // Corrupt the session metadata by writing garbage
        let write_txn = store.db.begin_write().expect("txn");
        {
            let mut table = write_txn.open_table(SESSION_META).expect("table");
            table
                .insert("data", &[0xFF, 0xFF, 0xFF][..])
                .expect("insert");
        }
        write_txn.commit().expect("commit");

        // Should return None instead of erroring
        let result = store.load_session().expect("should not error");
        assert!(
            result.is_none(),
            "Should return None for corrupted session data"
        );
    }
}
