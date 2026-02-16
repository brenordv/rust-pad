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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SessionTabEntry {
    /// A file-backed tab (just needs its path to reopen).
    File { path: String },
    /// An unsaved/untitled tab whose content is stored in the session DB.
    Unsaved { session_id: String, title: String },
}

/// The full session state: ordered list of tabs + which one was active.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionData {
    pub tabs: Vec<SessionTabEntry>,
    pub active_tab_index: usize,
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
    /// Returns the default session database path (next to the executable).
    pub fn session_path() -> PathBuf {
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.join("rust-pad-session.redb")))
            .unwrap_or_else(|| PathBuf::from("rust-pad-session.redb"))
    }

    /// Opens or creates the session database at `path`.
    pub fn open(path: &Path) -> Result<Self> {
        let db = Database::create(path)
            .with_context(|| format!("Failed to open session database: {}", path.display()))?;

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

    /// Saves the session tab list and active index.
    pub fn save_session(&self, data: &SessionData) -> Result<()> {
        let bytes = bincode::serialize(data).context("Failed to serialize session data")?;

        let write_txn = self
            .db
            .begin_write()
            .context("Failed to begin write transaction")?;
        {
            let mut table = write_txn
                .open_table(SESSION_META)
                .context("Failed to open session_meta table")?;
            table
                .insert("data", bytes.as_slice())
                .context("Failed to insert session data")?;
        }
        write_txn
            .commit()
            .context("Failed to commit session data")?;
        Ok(())
    }

    /// Loads the session tab list, or `None` if no session was saved.
    pub fn load_session(&self) -> Result<Option<SessionData>> {
        let read_txn = self
            .db
            .begin_read()
            .context("Failed to begin read transaction")?;
        let table = read_txn
            .open_table(SESSION_META)
            .context("Failed to open session_meta table")?;

        match table.get("data").context("Failed to read session data")? {
            Some(guard) => {
                let data: SessionData = bincode::deserialize(guard.value())
                    .context("Failed to deserialize session data")?;
                Ok(Some(data))
            }
            None => Ok(None),
        }
    }

    /// Saves the content of an unsaved tab.
    pub fn save_content(&self, session_id: &str, content: &str) -> Result<()> {
        let write_txn = self
            .db
            .begin_write()
            .context("Failed to begin write transaction")?;
        {
            let mut table = write_txn
                .open_table(SESSION_CONTENT)
                .context("Failed to open session_content table")?;
            table
                .insert(session_id, content)
                .context("Failed to insert session content")?;
        }
        write_txn
            .commit()
            .context("Failed to commit session content")?;
        Ok(())
    }

    /// Loads the content of an unsaved tab, or `None` if not found.
    pub fn load_content(&self, session_id: &str) -> Result<Option<String>> {
        let read_txn = self
            .db
            .begin_read()
            .context("Failed to begin read transaction")?;
        let table = read_txn
            .open_table(SESSION_CONTENT)
            .context("Failed to open session_content table")?;

        match table
            .get(session_id)
            .context("Failed to read session content")?
        {
            Some(guard) => Ok(Some(guard.value().to_string())),
            None => Ok(None),
        }
    }

    /// Deletes the content for one unsaved tab (cleanup on tab close).
    pub fn delete_content(&self, session_id: &str) -> Result<()> {
        let write_txn = self
            .db
            .begin_write()
            .context("Failed to begin write transaction")?;
        {
            let mut table = write_txn
                .open_table(SESSION_CONTENT)
                .context("Failed to open session_content table")?;
            let _ = table.remove(session_id);
        }
        write_txn
            .commit()
            .context("Failed to commit content deletion")?;
        Ok(())
    }

    /// Wipes all stored content entries (used on startup after restoring).
    pub fn clear_all_content(&self) -> Result<()> {
        let write_txn = self
            .db
            .begin_write()
            .context("Failed to begin write transaction")?;
        {
            let mut table = write_txn
                .open_table(SESSION_CONTENT)
                .context("Failed to open session_content table")?;

            // Collect all keys then remove them
            let keys: Vec<String> = table
                .iter()
                .context("Failed to iterate session_content")?
                .filter_map(|entry| entry.ok().map(|(k, _)| k.value().to_string()))
                .collect();

            for key in &keys {
                let _ = table.remove(key.as_str());
            }
        }
        write_txn
            .commit()
            .context("Failed to commit content clear")?;
        Ok(())
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
                },
                SessionTabEntry::Unsaved {
                    session_id: "sess-0".to_string(),
                    title: "Untitled".to_string(),
                },
            ],
            active_tab_index: 1,
        };

        store.save_session(&data).expect("save");
        let loaded = store.load_session().expect("load").expect("some");

        assert_eq!(loaded.tabs.len(), 2);
        assert_eq!(loaded.active_tab_index, 1);

        match &loaded.tabs[0] {
            SessionTabEntry::File { path } => assert_eq!(path, "/tmp/foo.rs"),
            _ => panic!("expected File entry"),
        }
        match &loaded.tabs[1] {
            SessionTabEntry::Unsaved { session_id, title } => {
                assert_eq!(session_id, "sess-0");
                assert_eq!(title, "Untitled");
            }
            _ => panic!("expected Unsaved entry"),
        }
    }

    #[test]
    fn test_save_and_load_content() {
        let (store, _dir) = open_test_store();

        store
            .save_content("sess-1", "Hello, world!\nLine 2\n\tTabbed")
            .expect("save");
        let loaded = store.load_content("sess-1").expect("load").expect("some");
        assert_eq!(loaded, "Hello, world!\nLine 2\n\tTabbed");
    }

    #[test]
    fn test_delete_content() {
        let (store, _dir) = open_test_store();

        store.save_content("sess-2", "some text").expect("save");
        assert!(store.load_content("sess-2").expect("load").is_some());

        store.delete_content("sess-2").expect("delete");
        assert!(store.load_content("sess-2").expect("load").is_none());
    }

    #[test]
    fn test_clear_all_content() {
        let (store, _dir) = open_test_store();

        store.save_content("a", "content-a").expect("save a");
        store.save_content("b", "content-b").expect("save b");
        store.save_content("c", "content-c").expect("save c");

        store.clear_all_content().expect("clear");

        assert!(store.load_content("a").expect("load").is_none());
        assert!(store.load_content("b").expect("load").is_none());
        assert!(store.load_content("c").expect("load").is_none());
    }

    #[test]
    fn test_session_tab_entry_serde() {
        let file_entry = SessionTabEntry::File {
            path: "test.txt".to_string(),
        };
        let unsaved_entry = SessionTabEntry::Unsaved {
            session_id: "sess-42".to_string(),
            title: "My Tab".to_string(),
        };

        // bincode round-trip
        let bytes1 = bincode::serialize(&file_entry).expect("serialize");
        let decoded1: SessionTabEntry = bincode::deserialize(&bytes1).expect("deserialize");
        match decoded1 {
            SessionTabEntry::File { path } => assert_eq!(path, "test.txt"),
            _ => panic!("expected File"),
        }

        let bytes2 = bincode::serialize(&unsaved_entry).expect("serialize");
        let decoded2: SessionTabEntry = bincode::deserialize(&bytes2).expect("deserialize");
        match decoded2 {
            SessionTabEntry::Unsaved { session_id, title } => {
                assert_eq!(session_id, "sess-42");
                assert_eq!(title, "My Tab");
            }
            _ => panic!("expected Unsaved"),
        }
    }

    #[test]
    fn test_content_with_special_chars() {
        let (store, _dir) = open_test_store();

        // Test with unicode, quotes, backslashes, null-like patterns
        let content = "Hello 🌍\n\"quotes\" and \\backslash\n\t\ttabs\nline with \0 null";
        store.save_content("special", content).expect("save");
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
}
