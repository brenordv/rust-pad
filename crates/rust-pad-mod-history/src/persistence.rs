/// Disk persistence layer backed by redb.
///
/// Uses a single redb database file with two tables:
/// - `history`: stores serialized `EditGroup` entries keyed by `"{doc_id}#{seq:020}"`
/// - `meta`: stores per-document metadata keyed by `doc_id`
use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result};
use redb::{Database, ReadableDatabase, ReadableTable, TableDefinition};

use crate::operation::EditGroup;

/// History table: composite string key → bincode-serialized EditGroup.
const HISTORY_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("history");

/// Metadata table: doc_id → bincode-serialized DocumentMeta.
const META_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("meta");

/// Per-document metadata persisted alongside history.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct DocumentMeta {
    next_seq: u64,
}

/// Formats a history table key from doc_id and sequence number.
///
/// The sequence number is zero-padded to 20 digits to ensure correct
/// lexicographic ordering in the B-tree.
fn history_key(doc_id: &str, seq: u64) -> String {
    format!("{doc_id}#{seq:020}")
}

/// Returns the exclusive range bounds for all history entries of a document.
///
/// Uses `#` as separator and `$` (one ASCII codepoint above `#`) as the
/// exclusive upper bound, ensuring the range captures exactly the entries
/// for the given doc_id.
fn doc_range(doc_id: &str) -> (String, String) {
    let start = format!("{doc_id}#");
    let end = format!("{doc_id}$");
    (start, end)
}

/// Persistence layer for undo/redo history backed by redb.
///
/// Thread-safe: redb supports concurrent readers and serialized writers.
/// Shared across documents via `Arc<PersistenceLayer>`.
pub struct PersistenceLayer {
    db: Database,
}

impl std::fmt::Debug for PersistenceLayer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PersistenceLayer").finish()
    }
}

impl PersistenceLayer {
    /// Opens or creates the history database in the given directory.
    ///
    /// Creates the directory and database file if they don't exist.
    /// Initializes tables on first use.
    ///
    /// # Errors
    ///
    /// Returns an error if the directory cannot be created or the database
    /// cannot be opened.
    pub fn open(data_dir: &Path) -> Result<Arc<Self>> {
        std::fs::create_dir_all(data_dir)
            .with_context(|| format!("Failed to create data directory: {}", data_dir.display()))?;

        let db_path = data_dir.join("history.redb");
        let db = Database::create(&db_path)
            .with_context(|| format!("Failed to open history database: {}", db_path.display()))?;

        // Ensure tables exist
        let write_txn = db
            .begin_write()
            .context("Failed to begin initial write transaction")?;
        {
            let _ = write_txn
                .open_table(HISTORY_TABLE)
                .context("Failed to create history table")?;
            let _ = write_txn
                .open_table(META_TABLE)
                .context("Failed to create meta table")?;
        }
        write_txn
            .commit()
            .context("Failed to commit initial transaction")?;

        Ok(Arc::new(Self { db }))
    }

    /// Writes edit groups to disk for a document.
    ///
    /// Uses upsert semantics: existing entries with the same key are overwritten.
    ///
    /// # Errors
    ///
    /// Returns an error if the write transaction fails.
    pub fn write_groups(&self, doc_id: &str, groups: &[EditGroup]) -> Result<()> {
        if groups.is_empty() {
            return Ok(());
        }

        let write_txn = self
            .db
            .begin_write()
            .context("Failed to begin write transaction")?;
        {
            let mut table = write_txn
                .open_table(HISTORY_TABLE)
                .context("Failed to open history table")?;
            for group in groups {
                let key = history_key(doc_id, group.seq);
                let bytes = bincode::serialize(group).context("Failed to serialize edit group")?;
                table
                    .insert(key.as_str(), bytes.as_slice())
                    .context("Failed to insert edit group")?;
            }
        }
        write_txn
            .commit()
            .context("Failed to commit write transaction")?;
        Ok(())
    }

    /// Reads all edit groups for a document, ordered by sequence number.
    ///
    /// # Errors
    ///
    /// Returns an error if the read transaction or deserialization fails.
    pub fn read_groups(&self, doc_id: &str) -> Result<Vec<EditGroup>> {
        let read_txn = self
            .db
            .begin_read()
            .context("Failed to begin read transaction")?;
        let table = read_txn
            .open_table(HISTORY_TABLE)
            .context("Failed to open history table")?;

        let (start, end) = doc_range(doc_id);
        let mut groups = Vec::new();

        for entry in table
            .range::<&str>(start.as_str()..end.as_str())
            .context("Failed to range query history table")?
        {
            let (_, value_guard) = entry.context("Failed to read history entry")?;
            let group: EditGroup = bincode::deserialize(value_guard.value())
                .context("Failed to deserialize edit group")?;
            groups.push(group);
        }

        Ok(groups)
    }

    /// Counts the number of edit groups stored for a document.
    ///
    /// # Errors
    ///
    /// Returns an error if the read transaction fails.
    pub fn count_groups(&self, doc_id: &str) -> Result<usize> {
        let read_txn = self
            .db
            .begin_read()
            .context("Failed to begin read transaction")?;
        let table = read_txn
            .open_table(HISTORY_TABLE)
            .context("Failed to open history table")?;

        let (start, end) = doc_range(doc_id);
        let count = table
            .range::<&str>(start.as_str()..end.as_str())
            .context("Failed to range query for count")?
            .count();

        Ok(count)
    }

    /// Removes the `count` oldest groups for a document.
    ///
    /// # Errors
    ///
    /// Returns an error if the write transaction fails.
    pub fn evict_oldest(&self, doc_id: &str, count: usize) -> Result<usize> {
        if count == 0 {
            return Ok(0);
        }

        let write_txn = self
            .db
            .begin_write()
            .context("Failed to begin write transaction")?;
        let mut evicted = 0;
        {
            let mut table = write_txn
                .open_table(HISTORY_TABLE)
                .context("Failed to open history table")?;

            let (start, end) = doc_range(doc_id);
            let keys_to_remove: Vec<String> = table
                .range::<&str>(start.as_str()..end.as_str())
                .context("Failed to range query for eviction")?
                .take(count)
                .filter_map(|entry| entry.ok().map(|(k, _)| k.value().to_string()))
                .collect();

            for key in &keys_to_remove {
                table
                    .remove(key.as_str())
                    .context("Failed to remove evicted entry")?;
                evicted += 1;
            }
        }
        write_txn.commit().context("Failed to commit eviction")?;
        Ok(evicted)
    }

    /// Removes all history and metadata for a document.
    ///
    /// # Errors
    ///
    /// Returns an error if the write transaction fails.
    pub fn delete_document(&self, doc_id: &str) -> Result<()> {
        let write_txn = self
            .db
            .begin_write()
            .context("Failed to begin write transaction")?;
        {
            let mut table = write_txn
                .open_table(HISTORY_TABLE)
                .context("Failed to open history table")?;

            let (start, end) = doc_range(doc_id);
            let keys_to_remove: Vec<String> = table
                .range::<&str>(start.as_str()..end.as_str())
                .context("Failed to range query for deletion")?
                .filter_map(|entry| entry.ok().map(|(k, _)| k.value().to_string()))
                .collect();

            for key in &keys_to_remove {
                table
                    .remove(key.as_str())
                    .context("Failed to remove entry")?;
            }
        }
        {
            let mut meta_table = write_txn
                .open_table(META_TABLE)
                .context("Failed to open meta table")?;
            let _ = meta_table.remove(doc_id);
        }
        write_txn.commit().context("Failed to commit deletion")?;
        Ok(())
    }

    /// Saves the next sequence number for a document.
    ///
    /// # Errors
    ///
    /// Returns an error if the write transaction fails.
    pub fn save_meta(&self, doc_id: &str, next_seq: u64) -> Result<()> {
        let meta = DocumentMeta { next_seq };
        let bytes = bincode::serialize(&meta).context("Failed to serialize document metadata")?;

        let write_txn = self
            .db
            .begin_write()
            .context("Failed to begin write transaction")?;
        {
            let mut table = write_txn
                .open_table(META_TABLE)
                .context("Failed to open meta table")?;
            table
                .insert(doc_id, bytes.as_slice())
                .context("Failed to insert metadata")?;
        }
        write_txn.commit().context("Failed to commit metadata")?;
        Ok(())
    }

    /// Loads the next sequence number for a document.
    ///
    /// Returns `None` if no history exists for this document.
    ///
    /// # Errors
    ///
    /// Returns an error if the read transaction or deserialization fails.
    pub fn load_meta(&self, doc_id: &str) -> Result<Option<u64>> {
        let read_txn = self
            .db
            .begin_read()
            .context("Failed to begin read transaction")?;
        let table = read_txn
            .open_table(META_TABLE)
            .context("Failed to open meta table")?;

        match table.get(doc_id).context("Failed to read metadata")? {
            Some(guard) => {
                let meta: DocumentMeta = bincode::deserialize(guard.value())
                    .context("Failed to deserialize metadata")?;
                Ok(Some(meta.next_seq))
            }
            None => Ok(None),
        }
    }

    /// Lists all document IDs that have stored metadata.
    ///
    /// # Errors
    ///
    /// Returns an error if the read transaction fails.
    pub fn list_documents(&self) -> Result<Vec<String>> {
        let read_txn = self
            .db
            .begin_read()
            .context("Failed to begin read transaction")?;
        let table = read_txn
            .open_table(META_TABLE)
            .context("Failed to open meta table")?;

        let mut doc_ids = Vec::new();
        for entry in table.iter().context("Failed to iterate meta table")? {
            let (key_guard, _) = entry.context("Failed to read meta entry")?;
            doc_ids.push(key_guard.value().to_string());
        }
        Ok(doc_ids)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::operation::{CursorSnapshot, EditOperation};
    use tempfile::TempDir;

    fn make_group(seq: u64, text: &str) -> EditGroup {
        EditGroup {
            operations: vec![EditOperation {
                position: 0,
                inserted: text.to_string(),
                deleted: String::new(),
                cursor_before: CursorSnapshot::default(),
                cursor_after: CursorSnapshot::default(),
            }],
            seq,
        }
    }

    fn open_test_db() -> (Arc<PersistenceLayer>, TempDir) {
        let dir = TempDir::new().expect("create temp dir");
        let pl = PersistenceLayer::open(dir.path()).expect("open db");
        (pl, dir)
    }

    #[test]
    fn test_open_creates_database() {
        let (pl, _dir) = open_test_db();
        let docs = pl.list_documents().expect("list docs");
        assert!(docs.is_empty());
    }

    #[test]
    fn test_write_and_read_groups() {
        let (pl, _dir) = open_test_db();
        let doc_id = "test-doc-1";

        let groups = vec![make_group(0, "a"), make_group(1, "b"), make_group(2, "c")];
        pl.write_groups(doc_id, &groups).expect("write");

        let loaded = pl.read_groups(doc_id).expect("read");
        assert_eq!(loaded.len(), 3);
        assert_eq!(loaded[0].seq, 0);
        assert_eq!(loaded[1].seq, 1);
        assert_eq!(loaded[2].seq, 2);
        assert_eq!(loaded[0].operations[0].inserted, "a");
    }

    #[test]
    fn test_write_empty_groups_is_noop() {
        let (pl, _dir) = open_test_db();
        pl.write_groups("doc", &[]).expect("write empty");
        let loaded = pl.read_groups("doc").expect("read");
        assert!(loaded.is_empty());
    }

    #[test]
    fn test_upsert_overwrites_existing() {
        let (pl, _dir) = open_test_db();
        let doc_id = "test-doc";

        pl.write_groups(doc_id, &[make_group(0, "original")])
            .expect("write");
        pl.write_groups(doc_id, &[make_group(0, "updated")])
            .expect("overwrite");

        let loaded = pl.read_groups(doc_id).expect("read");
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].operations[0].inserted, "updated");
    }

    #[test]
    fn test_count_groups() {
        let (pl, _dir) = open_test_db();
        let doc_id = "count-doc";

        assert_eq!(pl.count_groups(doc_id).expect("count"), 0);

        let groups: Vec<EditGroup> = (0..5).map(|i| make_group(i, "x")).collect();
        pl.write_groups(doc_id, &groups).expect("write");

        assert_eq!(pl.count_groups(doc_id).expect("count"), 5);
    }

    #[test]
    fn test_evict_oldest() {
        let (pl, _dir) = open_test_db();
        let doc_id = "evict-doc";

        let groups: Vec<EditGroup> = (0..10).map(|i| make_group(i, &format!("g{i}"))).collect();
        pl.write_groups(doc_id, &groups).expect("write");

        let evicted = pl.evict_oldest(doc_id, 3).expect("evict");
        assert_eq!(evicted, 3);

        let remaining = pl.read_groups(doc_id).expect("read");
        assert_eq!(remaining.len(), 7);
        assert_eq!(remaining[0].seq, 3);
    }

    #[test]
    fn test_evict_more_than_exists() {
        let (pl, _dir) = open_test_db();
        let doc_id = "evict-all";

        let groups: Vec<EditGroup> = (0..3).map(|i| make_group(i, "x")).collect();
        pl.write_groups(doc_id, &groups).expect("write");

        let evicted = pl.evict_oldest(doc_id, 100).expect("evict");
        assert_eq!(evicted, 3);

        let remaining = pl.read_groups(doc_id).expect("read");
        assert!(remaining.is_empty());
    }

    #[test]
    fn test_delete_document() {
        let (pl, _dir) = open_test_db();
        let doc_id = "delete-doc";

        pl.write_groups(doc_id, &[make_group(0, "a")])
            .expect("write");
        pl.save_meta(doc_id, 1).expect("save meta");

        pl.delete_document(doc_id).expect("delete");

        assert!(pl.read_groups(doc_id).expect("read").is_empty());
        assert!(pl.load_meta(doc_id).expect("meta").is_none());
    }

    #[test]
    fn test_save_and_load_meta() {
        let (pl, _dir) = open_test_db();
        let doc_id = "meta-doc";

        assert!(pl.load_meta(doc_id).expect("load").is_none());

        pl.save_meta(doc_id, 42).expect("save");
        let next_seq = pl.load_meta(doc_id).expect("load").expect("exists");
        assert_eq!(next_seq, 42);

        pl.save_meta(doc_id, 100).expect("update");
        let next_seq = pl.load_meta(doc_id).expect("load").expect("exists");
        assert_eq!(next_seq, 100);
    }

    #[test]
    fn test_multi_document_isolation() {
        let (pl, _dir) = open_test_db();

        pl.write_groups("doc-a", &[make_group(0, "a1"), make_group(1, "a2")])
            .expect("write a");
        pl.write_groups("doc-b", &[make_group(0, "b1")])
            .expect("write b");

        let a_groups = pl.read_groups("doc-a").expect("read a");
        let b_groups = pl.read_groups("doc-b").expect("read b");
        assert_eq!(a_groups.len(), 2);
        assert_eq!(b_groups.len(), 1);
        assert_eq!(b_groups[0].operations[0].inserted, "b1");

        pl.delete_document("doc-a").expect("delete a");
        assert!(pl.read_groups("doc-a").expect("read a").is_empty());
        assert_eq!(pl.read_groups("doc-b").expect("read b").len(), 1);
    }

    #[test]
    fn test_list_documents() {
        let (pl, _dir) = open_test_db();

        pl.save_meta("doc-x", 1).expect("save");
        pl.save_meta("doc-y", 2).expect("save");

        let mut docs = pl.list_documents().expect("list");
        docs.sort();
        assert_eq!(docs, vec!["doc-x", "doc-y"]);
    }

    #[test]
    fn test_reopen_database_preserves_data() {
        let dir = TempDir::new().expect("create temp dir");

        // Write data
        {
            let pl = PersistenceLayer::open(dir.path()).expect("open");
            pl.write_groups("doc", &[make_group(0, "persistent")])
                .expect("write");
            pl.save_meta("doc", 1).expect("save meta");
        }

        // Reopen and verify
        {
            let pl = PersistenceLayer::open(dir.path()).expect("reopen");
            let groups = pl.read_groups("doc").expect("read");
            assert_eq!(groups.len(), 1);
            assert_eq!(groups[0].operations[0].inserted, "persistent");
            assert_eq!(pl.load_meta("doc").expect("meta").expect("exists"), 1);
        }
    }
}
