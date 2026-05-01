/// Problem log persistence: stores application error entries in a crash-safe
/// embedded database so that users can review errors from the Help > Problems
/// dialog even after a restart.
///
/// Each entry has a unique auto-incrementing ID, a UTC timestamp, a human-
/// readable message, and a read/unread flag. The database uses redb for
/// crash safety — entries survive even if the process terminates unexpectedly.
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{Context, Result};
use redb::{Database, ReadableDatabase, ReadableTable, TableDefinition};

use crate::db_helpers::{open_or_create_db, read_table, write_table};

/// Table layout: `u64` (entry ID) → `&[u8]` (4-field record, see below).
///
/// Each value is a simple length-prefixed format:
///   - 8 bytes: timestamp (i64 big-endian, seconds since Unix epoch)
///   - 1 byte:  read flag (0 = unread, 1 = read)
///   - 4 bytes: message length (u32 big-endian)
///   - N bytes: message (UTF-8)
const PROBLEM_TABLE: TableDefinition<u64, &[u8]> = TableDefinition::new("problems");

/// A single problem log entry.
#[derive(Debug, Clone)]
pub struct ProblemEntry {
    /// Unique, monotonically increasing identifier.
    pub id: u64,
    /// UTC timestamp as seconds since Unix epoch.
    pub timestamp: i64,
    /// Human-readable error description.
    pub message: String,
    /// Whether the user has marked this entry as read.
    pub read: bool,
}

/// Persistence layer for problem log entries, backed by redb.
pub struct ProblemStore {
    db: Database,
    /// Per-instance monotonic ID counter, seeded from the DB on open.
    next_id: AtomicU64,
}

impl std::fmt::Debug for ProblemStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProblemStore")
            .field("next_id", &self.next_id.load(Ordering::Relaxed))
            .finish()
    }
}

// ── Serialization helpers ─────────────────────────────────────────

fn encode_entry(timestamp: i64, read: bool, message: &str) -> Vec<u8> {
    let msg_bytes = message.as_bytes();
    let msg_len = msg_bytes.len() as u32;
    let mut buf = Vec::with_capacity(8 + 1 + 4 + msg_bytes.len());
    buf.extend_from_slice(&timestamp.to_be_bytes());
    buf.push(u8::from(read));
    buf.extend_from_slice(&msg_len.to_be_bytes());
    buf.extend_from_slice(msg_bytes);
    buf
}

fn decode_entry(id: u64, bytes: &[u8]) -> Option<ProblemEntry> {
    if bytes.len() < 13 {
        return None;
    }
    let timestamp = i64::from_be_bytes(bytes[0..8].try_into().ok()?);
    let read = bytes[8] != 0;
    let msg_len = u32::from_be_bytes(bytes[9..13].try_into().ok()?) as usize;
    if bytes.len() < 13 + msg_len {
        return None;
    }
    let message = String::from_utf8_lossy(&bytes[13..13 + msg_len]).into_owned();
    Some(ProblemEntry {
        id,
        timestamp,
        message,
        read,
    })
}

/// Returns `true` when raw entry bytes represent an unread entry (byte 8 == 0).
fn is_unread_bytes(bytes: &[u8]) -> bool {
    bytes.len() > 8 && bytes[8] == 0
}

impl ProblemStore {
    /// Returns the default problem-log database path in the platform-standard
    /// data directory.
    pub fn default_path() -> std::path::PathBuf {
        crate::paths::problem_log_file_path()
    }

    /// Opens or creates the problem-log database at `path`.
    ///
    /// Creates the parent directory if it does not exist. Seeds the in-process
    /// ID counter from the maximum existing entry ID so that new entries never
    /// collide with old ones.
    pub fn open(path: &Path) -> Result<Self> {
        let db = open_or_create_db(path, "problem-log")?;

        // Ensure the table exists.
        let write_txn = db
            .begin_write()
            .context("Failed to begin initial problem-log write transaction")?;
        {
            let _ = write_txn
                .open_table(PROBLEM_TABLE)
                .context("Failed to create problems table")?;
        }
        write_txn
            .commit()
            .context("Failed to commit initial problem-log transaction")?;

        // Seed the ID counter from the highest existing key.
        let read_txn = db
            .begin_read()
            .context("Failed to begin read transaction for ID seed")?;
        let table = read_txn
            .open_table(PROBLEM_TABLE)
            .context("Failed to open problems table for ID seed")?;
        let max_id = table
            .iter()
            .ok()
            .and_then(|mut iter| iter.next_back())
            .and_then(|entry| entry.ok().map(|(k, _)| k.value()))
            .unwrap_or(0);

        Ok(Self {
            db,
            next_id: AtomicU64::new(max_id + 1),
        })
    }

    /// Records a new problem entry with the current UTC timestamp.
    pub fn add_entry(&self, message: &str) -> Result<()> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let timestamp = chrono::Utc::now().timestamp();
        let bytes = encode_entry(timestamp, false, message);

        write_table!(self.db, PROBLEM_TABLE, |table| {
            table
                .insert(id, bytes.as_slice())
                .context("Failed to insert problem entry")?;
            Ok(())
        })
    }

    /// Returns all entries ordered by timestamp descending (newest first).
    pub fn load_all(&self) -> Result<Vec<ProblemEntry>> {
        read_table!(self.db, PROBLEM_TABLE, |table| {
            let mut entries = Vec::new();
            for item in table.iter().context("Failed to iterate problems")? {
                let (key, value) = item.context("Failed to read problem entry")?;
                if let Some(entry) = decode_entry(key.value(), value.value()) {
                    entries.push(entry);
                }
            }
            // Sort by timestamp descending (newest first), then by id descending
            // as a tiebreaker.
            entries.sort_by(|a, b| b.timestamp.cmp(&a.timestamp).then(b.id.cmp(&a.id)));
            Ok(entries)
        })
    }

    /// Marks a single entry as read.
    pub fn mark_as_read(&self, id: u64) -> Result<()> {
        write_table!(self.db, PROBLEM_TABLE, |table| {
            // Read existing bytes into an owned Vec, then drop the guard
            // before mutably borrowing the table for the update.
            let old_bytes = {
                let guard = table.get(id).context("Failed to read problem entry")?;
                guard.map(|g| g.value().to_vec())
            };

            if let Some(old_bytes) = old_bytes {
                if let Some(entry) = decode_entry(id, &old_bytes) {
                    if !entry.read {
                        let new_bytes = encode_entry(entry.timestamp, true, &entry.message);
                        table
                            .insert(id, new_bytes.as_slice())
                            .context("Failed to update problem entry")?;
                    }
                }
            }
            Ok(())
        })
    }

    /// Marks all entries as read.
    pub fn mark_all_as_read(&self) -> Result<()> {
        write_table!(self.db, PROBLEM_TABLE, |table| {
            // Collect entries that need updating.
            let unread: Vec<(u64, Vec<u8>)> = table
                .iter()
                .context("Failed to iterate problems")?
                .filter_map(|item| {
                    let (k, v) = item.ok()?;
                    let id = k.value();
                    let bytes: Vec<u8> = v.value().to_vec();
                    if is_unread_bytes(&bytes) {
                        Some((id, bytes))
                    } else {
                        None
                    }
                })
                .collect();

            for (id, old_bytes) in &unread {
                if let Some(entry) = decode_entry(*id, old_bytes) {
                    let new_bytes = encode_entry(entry.timestamp, true, &entry.message);
                    table
                        .insert(*id, new_bytes.as_slice())
                        .context("Failed to update problem entry")?;
                }
            }
            Ok(())
        })
    }

    /// Returns the number of unread entries.
    pub fn unread_count(&self) -> Result<usize> {
        read_table!(self.db, PROBLEM_TABLE, |table| {
            let mut count = 0usize;
            for (_, v) in table
                .iter()
                .context("Failed to iterate problems")?
                .flatten()
            {
                if is_unread_bytes(v.value()) {
                    count += 1;
                }
            }
            Ok(count)
        })
    }

    /// Deletes all entries from the problem log.
    pub fn clear_all(&self) -> Result<()> {
        write_table!(self.db, PROBLEM_TABLE, |table| {
            let keys: Vec<u64> = table
                .iter()
                .context("Failed to iterate problems")?
                .filter_map(|item| item.ok().map(|(k, _)| k.value()))
                .collect();

            for key in &keys {
                let _ = table.remove(*key);
            }
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn open_test_store() -> (ProblemStore, TempDir) {
        let dir = TempDir::new().expect("create temp dir");
        let db_path = dir.path().join("test-problems.redb");
        let store = ProblemStore::open(&db_path).expect("open problem store");
        (store, dir)
    }

    #[test]
    fn test_empty_store() {
        let (store, _dir) = open_test_store();
        let entries = store.load_all().expect("load");
        assert!(entries.is_empty());
        assert_eq!(store.unread_count().expect("count"), 0);
    }

    #[test]
    fn test_add_and_load_entries() {
        let (store, _dir) = open_test_store();

        store.add_entry("Error one").expect("add");
        store.add_entry("Error two").expect("add");
        store.add_entry("Error three").expect("add");

        let entries = store.load_all().expect("load");
        assert_eq!(entries.len(), 3);
        // Newest first
        assert_eq!(entries[0].message, "Error three");
        assert!(!entries[0].read);
        assert_eq!(store.unread_count().expect("count"), 3);
    }

    #[test]
    fn test_mark_as_read() {
        let (store, _dir) = open_test_store();

        store.add_entry("Error A").expect("add");
        store.add_entry("Error B").expect("add");

        let entries = store.load_all().expect("load");
        let id = entries[0].id;

        store.mark_as_read(id).expect("mark");
        let updated = store.load_all().expect("load");
        let marked = updated.iter().find(|e| e.id == id).expect("find");
        assert!(marked.read);
        assert_eq!(store.unread_count().expect("count"), 1);
    }

    #[test]
    fn test_mark_all_as_read() {
        let (store, _dir) = open_test_store();

        store.add_entry("Error X").expect("add");
        store.add_entry("Error Y").expect("add");
        store.add_entry("Error Z").expect("add");

        store.mark_all_as_read().expect("mark all");
        assert_eq!(store.unread_count().expect("count"), 0);

        let entries = store.load_all().expect("load");
        assert!(entries.iter().all(|e| e.read));
    }

    #[test]
    fn test_clear_all() {
        let (store, _dir) = open_test_store();

        store.add_entry("Error 1").expect("add");
        store.add_entry("Error 2").expect("add");

        store.clear_all().expect("clear");
        let entries = store.load_all().expect("load");
        assert!(entries.is_empty());
    }

    #[test]
    fn test_encode_decode_roundtrip() {
        let timestamp = 1700000000i64;
        let message = "Something went wrong: file not found 🔍";
        let bytes = encode_entry(timestamp, false, message);
        let entry = decode_entry(42, &bytes).expect("decode");
        assert_eq!(entry.id, 42);
        assert_eq!(entry.timestamp, timestamp);
        assert_eq!(entry.message, message);
        assert!(!entry.read);
    }

    #[test]
    fn test_decode_short_bytes_returns_none() {
        assert!(decode_entry(1, &[0; 5]).is_none());
    }

    #[test]
    fn test_persistence_across_reopen() {
        let dir = TempDir::new().expect("create temp dir");
        let db_path = dir.path().join("test-persist.redb");

        // Write entries
        {
            let store = ProblemStore::open(&db_path).expect("open");
            store.add_entry("Persisted error").expect("add");
        }

        // Reopen and verify
        {
            let store = ProblemStore::open(&db_path).expect("reopen");
            let entries = store.load_all().expect("load");
            assert_eq!(entries.len(), 1);
            assert_eq!(entries[0].message, "Persisted error");
        }
    }

    #[test]
    fn test_mark_as_read_nonexistent_id() {
        let (store, _dir) = open_test_store();
        // Marking a nonexistent ID should succeed without error.
        store.mark_as_read(9999).expect("mark nonexistent");
        assert_eq!(store.unread_count().expect("count"), 0);
    }

    #[test]
    fn test_mark_as_read_idempotent() {
        let (store, _dir) = open_test_store();
        store.add_entry("Error").expect("add");

        let id = store.load_all().expect("load")[0].id;
        store.mark_as_read(id).expect("first mark");
        store.mark_as_read(id).expect("second mark");

        let entries = store.load_all().expect("load");
        assert!(entries[0].read);
        assert_eq!(store.unread_count().expect("count"), 0);
    }

    #[test]
    fn test_mark_all_as_read_empty_store() {
        let (store, _dir) = open_test_store();
        store.mark_all_as_read().expect("mark all on empty");
        assert_eq!(store.unread_count().expect("count"), 0);
    }

    #[test]
    fn test_clear_all_empty_store() {
        let (store, _dir) = open_test_store();
        store.clear_all().expect("clear empty");
        assert!(store.load_all().expect("load").is_empty());
    }

    #[test]
    fn test_unread_count_with_mixed_read_state() {
        let (store, _dir) = open_test_store();
        store.add_entry("A").expect("add");
        store.add_entry("B").expect("add");
        store.add_entry("C").expect("add");

        let entries = store.load_all().expect("load");
        store.mark_as_read(entries[0].id).expect("mark");

        assert_eq!(store.unread_count().expect("count"), 2);
    }

    #[test]
    fn test_encode_decode_read_flag_true() {
        let bytes = encode_entry(1700000000, true, "already read");
        let entry = decode_entry(1, &bytes).expect("decode");
        assert!(entry.read);
        assert_eq!(entry.message, "already read");
    }

    #[test]
    fn test_decode_truncated_message_returns_none() {
        // Header claims 100-byte message, but only 5 bytes follow.
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&0i64.to_be_bytes()); // timestamp
        bytes.push(0); // read flag
        bytes.extend_from_slice(&100u32.to_be_bytes()); // msg_len = 100
        bytes.extend_from_slice(b"short"); // only 5 bytes
        assert!(decode_entry(1, &bytes).is_none());
    }

    #[test]
    fn test_open_creates_parent_directories() {
        let dir = TempDir::new().expect("create temp dir");
        let nested = dir.path().join("a").join("b").join("c");
        let db_path = nested.join("problems.redb");
        let store = ProblemStore::open(&db_path).expect("open with nested dirs");
        store.add_entry("works").expect("add");
        assert_eq!(store.load_all().expect("load").len(), 1);
    }

    #[test]
    fn test_id_monotonicity_across_reopen() {
        let dir = TempDir::new().expect("create temp dir");
        let db_path = dir.path().join("test-ids.redb");

        let first_id;
        {
            let store = ProblemStore::open(&db_path).expect("open");
            store.add_entry("First").expect("add");
            first_id = store.load_all().expect("load")[0].id;
        }

        {
            let store = ProblemStore::open(&db_path).expect("reopen");
            store.add_entry("Second").expect("add");
            let entries = store.load_all().expect("load");
            let second_id = entries
                .iter()
                .find(|e| e.message == "Second")
                .expect("find")
                .id;
            assert!(second_id > first_id, "IDs should increase across reopens");
        }
    }
}
