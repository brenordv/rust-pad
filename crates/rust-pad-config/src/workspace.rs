/// Workspace persistence: stores named workspaces with their folder lists.
///
/// Each workspace groups a set of folder paths that are displayed in the
/// sidebar file explorer. The active workspace ID is stored separately so
/// it can be restored on the next launch.
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use redb::{Database, ReadableDatabase, TableDefinition};
use serde::{Deserialize, Serialize};

use crate::db_helpers::{deserialize_record, open_or_create_db, read_table, write_table};

/// Maximum size in bytes for deserializing workspace metadata.
/// 5 MB is generous for a list of workspace entries.
const MAX_WORKSPACE_META_BYTES: u64 = 5 * 1024 * 1024;

/// Workspace metadata table: key → bincode-encoded value.
/// Keys: "list" → Vec<WorkspaceEntry>, "active" → Option<String>.
const WORKSPACE_META: TableDefinition<&str, &[u8]> = TableDefinition::new("workspace_meta");

/// A single workspace definition.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkspaceEntry {
    /// Unique identifier (UUID v4).
    pub id: String,
    /// User-visible workspace name.
    pub name: String,
    /// Absolute paths of root folders in this workspace.
    pub folders: Vec<String>,
    /// ISO 8601 creation timestamp.
    pub created_at: String,
}

/// Persistence layer for workspace state, backed by redb.
pub struct WorkspaceStore {
    db: Database,
}

impl std::fmt::Debug for WorkspaceStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WorkspaceStore").finish()
    }
}

impl WorkspaceStore {
    /// Returns the default workspace database path in the platform-standard
    /// data directory.
    pub fn workspace_path() -> PathBuf {
        crate::paths::workspace_file_path()
    }

    /// Opens or creates the workspace database at `path`.
    ///
    /// Creates the parent directory if it does not exist.
    pub fn open(path: &Path) -> Result<Self> {
        let db = open_or_create_db(path, "workspace")?;

        // Ensure tables exist
        let write_txn = db
            .begin_write()
            .context("Failed to begin initial workspace write transaction")?;
        {
            let _ = write_txn
                .open_table(WORKSPACE_META)
                .context("Failed to create workspace_meta table")?;
        }
        write_txn
            .commit()
            .context("Failed to commit initial workspace transaction")?;

        Ok(Self { db })
    }

    /// Returns all saved workspaces.
    pub fn list_workspaces(&self) -> Result<Vec<WorkspaceEntry>> {
        read_table!(self.db, WORKSPACE_META, |table| {
            let entries = match table.get("list").context("Failed to read workspace list")? {
                Some(guard) => deserialize_record::<Vec<WorkspaceEntry>>(
                    guard.value(),
                    MAX_WORKSPACE_META_BYTES,
                    "workspace list",
                )
                .unwrap_or_default(),
                None => Vec::new(),
            };
            Ok(entries)
        })
    }

    /// Saves (upserts) a workspace entry. If a workspace with the same ID
    /// already exists, it is replaced.
    pub fn save_workspace(&self, entry: &WorkspaceEntry) -> Result<()> {
        let mut entries = self.list_workspaces()?;

        if let Some(existing) = entries.iter_mut().find(|e| e.id == entry.id) {
            *existing = entry.clone();
        } else {
            entries.push(entry.clone());
        }

        let bytes = bincode::serialize(&entries).context("Failed to serialize workspace list")?;

        write_table!(self.db, WORKSPACE_META, |table| {
            table
                .insert("list", bytes.as_slice())
                .context("Failed to insert workspace list")?;
            Ok(())
        })
    }

    /// Deletes a workspace by ID.
    pub fn delete_workspace(&self, id: &str) -> Result<()> {
        let mut entries = self.list_workspaces()?;
        entries.retain(|e| e.id != id);

        let bytes = bincode::serialize(&entries).context("Failed to serialize workspace list")?;

        write_table!(self.db, WORKSPACE_META, |table| {
            table
                .insert("list", bytes.as_slice())
                .context("Failed to insert workspace list")?;
            Ok(())
        })
    }

    /// Returns the active workspace ID, or `None` if no workspace is active.
    pub fn get_active_workspace_id(&self) -> Result<Option<String>> {
        read_table!(self.db, WORKSPACE_META, |table| {
            let id = match table
                .get("active")
                .context("Failed to read active workspace")?
            {
                // `deserialize_record` returns `Option<T>`; the stored value
                // is itself an `Option<String>`. `unwrap_or(None)` flattens
                // both layers: corrupt record OR explicit `None` both yield
                // "no active workspace".
                Some(guard) => deserialize_record::<Option<String>>(
                    guard.value(),
                    MAX_WORKSPACE_META_BYTES,
                    "active workspace",
                )
                .unwrap_or(None),
                None => None,
            };
            Ok(id)
        })
    }

    /// Sets (or clears) the active workspace ID.
    pub fn set_active_workspace_id(&self, id: Option<&str>) -> Result<()> {
        let value: Option<String> = id.map(|s| s.to_string());
        let bytes =
            bincode::serialize(&value).context("Failed to serialize active workspace ID")?;

        write_table!(self.db, WORKSPACE_META, |table| {
            table
                .insert("active", bytes.as_slice())
                .context("Failed to insert active workspace ID")?;
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn open_test_store() -> (WorkspaceStore, TempDir) {
        let dir = TempDir::new().expect("create temp dir");
        let db_path = dir.path().join("test-workspace.redb");
        let store = WorkspaceStore::open(&db_path).expect("open workspace store");
        (store, dir)
    }

    fn make_entry(id: &str, name: &str, folders: &[&str]) -> WorkspaceEntry {
        WorkspaceEntry {
            id: id.to_string(),
            name: name.to_string(),
            folders: folders.iter().map(|s| s.to_string()).collect(),
            created_at: "2026-01-01T00:00:00Z".to_string(),
        }
    }

    #[test]
    fn test_empty_store_returns_empty_list() {
        let (store, _dir) = open_test_store();
        let result = store.list_workspaces().expect("list");
        assert!(result.is_empty());
    }

    #[test]
    fn test_save_and_list_workspace() {
        let (store, _dir) = open_test_store();

        let entry = make_entry("ws-1", "My Workspace", &["/home/user/project"]);
        store.save_workspace(&entry).expect("save");

        let entries = store.list_workspaces().expect("list");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].id, "ws-1");
        assert_eq!(entries[0].name, "My Workspace");
        assert_eq!(entries[0].folders, vec!["/home/user/project"]);
    }

    #[test]
    fn test_upsert_existing_workspace() {
        let (store, _dir) = open_test_store();

        let entry1 = make_entry("ws-1", "Original", &["/path/a"]);
        store.save_workspace(&entry1).expect("save");

        let entry2 = make_entry("ws-1", "Updated", &["/path/a", "/path/b"]);
        store.save_workspace(&entry2).expect("upsert");

        let entries = store.list_workspaces().expect("list");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "Updated");
        assert_eq!(entries[0].folders.len(), 2);
    }

    #[test]
    fn test_save_multiple_workspaces() {
        let (store, _dir) = open_test_store();

        store
            .save_workspace(&make_entry("ws-1", "First", &["/a"]))
            .expect("save 1");
        store
            .save_workspace(&make_entry("ws-2", "Second", &["/b"]))
            .expect("save 2");
        store
            .save_workspace(&make_entry("ws-3", "Third", &["/c"]))
            .expect("save 3");

        let entries = store.list_workspaces().expect("list");
        assert_eq!(entries.len(), 3);
    }

    #[test]
    fn test_delete_workspace() {
        let (store, _dir) = open_test_store();

        store
            .save_workspace(&make_entry("ws-1", "First", &["/a"]))
            .expect("save 1");
        store
            .save_workspace(&make_entry("ws-2", "Second", &["/b"]))
            .expect("save 2");

        store.delete_workspace("ws-1").expect("delete");

        let entries = store.list_workspaces().expect("list");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].id, "ws-2");
    }

    #[test]
    fn test_delete_nonexistent_workspace() {
        let (store, _dir) = open_test_store();

        store
            .save_workspace(&make_entry("ws-1", "First", &["/a"]))
            .expect("save");

        store.delete_workspace("nonexistent").expect("delete");

        let entries = store.list_workspaces().expect("list");
        assert_eq!(entries.len(), 1);
    }

    #[test]
    fn test_active_workspace_initially_none() {
        let (store, _dir) = open_test_store();
        let active = store.get_active_workspace_id().expect("get");
        assert!(active.is_none());
    }

    #[test]
    fn test_set_and_get_active_workspace() {
        let (store, _dir) = open_test_store();

        store.set_active_workspace_id(Some("ws-1")).expect("set");
        let active = store.get_active_workspace_id().expect("get");
        assert_eq!(active.as_deref(), Some("ws-1"));
    }

    #[test]
    fn test_clear_active_workspace() {
        let (store, _dir) = open_test_store();

        store.set_active_workspace_id(Some("ws-1")).expect("set");
        store.set_active_workspace_id(None).expect("clear");

        let active = store.get_active_workspace_id().expect("get");
        assert!(active.is_none());
    }

    #[test]
    fn test_corrupted_list_returns_empty() {
        let (store, _dir) = open_test_store();

        // Write garbage to the list key
        let write_txn = store.db.begin_write().expect("txn");
        {
            let mut table = write_txn.open_table(WORKSPACE_META).expect("table");
            table
                .insert("list", &[0xFF, 0xFF, 0xFF][..])
                .expect("insert");
        }
        write_txn.commit().expect("commit");

        let entries = store.list_workspaces().expect("should not error");
        assert!(entries.is_empty());
    }

    #[test]
    fn test_corrupted_active_returns_none() {
        let (store, _dir) = open_test_store();

        // Write garbage to the active key
        let write_txn = store.db.begin_write().expect("txn");
        {
            let mut table = write_txn.open_table(WORKSPACE_META).expect("table");
            table
                .insert("active", &[0xFF, 0xFF, 0xFF][..])
                .expect("insert");
        }
        write_txn.commit().expect("commit");

        let active = store.get_active_workspace_id().expect("should not error");
        assert!(active.is_none());
    }

    #[test]
    fn test_workspace_entry_serde_roundtrip() {
        let entry = make_entry("uuid-123", "Test WS", &["/foo/bar", "/baz"]);
        let bytes = bincode::serialize(&entry).expect("serialize");
        let decoded: WorkspaceEntry = bincode::deserialize(&bytes).expect("deserialize");
        assert_eq!(decoded, entry);
    }

    #[test]
    fn test_workspace_store_debug() {
        let (store, _dir) = open_test_store();
        let debug = format!("{store:?}");
        assert!(debug.contains("WorkspaceStore"));
    }

    #[test]
    fn test_overwrite_active_workspace() {
        let (store, _dir) = open_test_store();
        store.set_active_workspace_id(Some("ws-1")).expect("set 1");
        store.set_active_workspace_id(Some("ws-2")).expect("set 2");
        let active = store.get_active_workspace_id().expect("get");
        assert_eq!(active.as_deref(), Some("ws-2"));
    }

    #[test]
    fn test_delete_all_workspaces() {
        let (store, _dir) = open_test_store();
        store
            .save_workspace(&make_entry("ws-1", "A", &[]))
            .expect("save");
        store
            .save_workspace(&make_entry("ws-2", "B", &[]))
            .expect("save");
        store.delete_workspace("ws-1").expect("delete");
        store.delete_workspace("ws-2").expect("delete");
        let entries = store.list_workspaces().expect("list");
        assert!(entries.is_empty());
    }

    #[test]
    fn test_workspace_entry_with_multiple_folders() {
        let (store, _dir) = open_test_store();
        let entry = make_entry("ws-1", "Multi", &["/a", "/b", "/c"]);
        store.save_workspace(&entry).expect("save");
        let entries = store.list_workspaces().expect("list");
        assert_eq!(entries[0].folders.len(), 3);
    }

    #[test]
    fn test_workspace_entry_clone_and_eq() {
        let entry = make_entry("ws-1", "Test", &["/path"]);
        let cloned = entry.clone();
        assert_eq!(entry, cloned);
    }

    #[test]
    fn test_workspace_path_is_non_empty() {
        let path = WorkspaceStore::workspace_path();
        assert!(!path.as_os_str().is_empty());
    }

    #[test]
    fn test_save_workspace_with_empty_folders() {
        let (store, _dir) = open_test_store();
        let entry = make_entry("ws-empty", "Empty", &[]);
        store.save_workspace(&entry).expect("save");
        let entries = store.list_workspaces().expect("list");
        assert_eq!(entries.len(), 1);
        assert!(entries[0].folders.is_empty());
    }

    #[test]
    fn test_upsert_preserves_other_workspaces() {
        let (store, _dir) = open_test_store();
        store
            .save_workspace(&make_entry("ws-1", "First", &["/a"]))
            .expect("save");
        store
            .save_workspace(&make_entry("ws-2", "Second", &["/b"]))
            .expect("save");

        // Update only ws-1
        store
            .save_workspace(&make_entry("ws-1", "Updated First", &["/a", "/c"]))
            .expect("upsert");

        let entries = store.list_workspaces().expect("list");
        assert_eq!(entries.len(), 2);
        let ws1 = entries.iter().find(|e| e.id == "ws-1").unwrap();
        let ws2 = entries.iter().find(|e| e.id == "ws-2").unwrap();
        assert_eq!(ws1.name, "Updated First");
        assert_eq!(ws1.folders.len(), 2);
        assert_eq!(ws2.name, "Second");
        assert_eq!(ws2.folders.len(), 1);
    }
}
