/// Workspace lifecycle operations on the main App.
///
/// Handles creating, opening, closing, switching workspaces, and
/// adding/removing folders. Bridges the sidebar UI actions with the
/// persistence layer and filesystem watcher.
use std::path::{Path, PathBuf};

use rust_pad_config::{WorkspaceEntry, WorkspaceStore};

use super::App;
use crate::workspace::scanner::scan_directory;
use crate::workspace::sidebar::SidebarAction;
use crate::workspace::tree::{FolderRoot, TreeEntry};
use crate::workspace::watcher::{FsEvent, WorkspaceWatcher};

/// Generates a unique filename or folder name within `parent` that avoids collisions.
///
/// Given a base name like "new_file.txt", returns "new_file.txt" if unused,
/// otherwise "new_file 2.txt", "new_file 3.txt", etc.
/// For directories (is_dir=true), appends " 2", " 3", etc. without extension logic.
pub(crate) fn generate_unique_name(parent: &Path, base: &str, is_dir: bool) -> String {
    if !parent.join(base).exists() {
        return base.to_string();
    }
    let (stem, ext) = if is_dir {
        (base, "")
    } else {
        match base.rsplit_once('.') {
            Some((s, e)) => (s, e),
            None => (base, ""),
        }
    };
    for i in 2u32.. {
        let candidate = if ext.is_empty() {
            format!("{stem} {i}")
        } else {
            format!("{stem} {i}.{ext}")
        };
        if !parent.join(&candidate).exists() {
            return candidate;
        }
    }
    base.to_string()
}

/// Generates a unique workspace name by checking existing names.
///
/// Returns "New Workspace" if unused, otherwise "New Workspace 2", "New Workspace 3", etc.
fn generate_workspace_name(existing: &[WorkspaceEntry]) -> String {
    let base = "New Workspace";
    let names: Vec<&str> = existing.iter().map(|e| e.name.as_str()).collect();

    if !names.contains(&base) {
        return base.to_string();
    }

    for i in 2u32.. {
        let candidate = format!("{base} {i}");
        if !names.contains(&candidate.as_str()) {
            return candidate;
        }
    }

    // Unreachable in practice — u32::MAX candidates
    base.to_string()
}

/// Scans all workspace folders and creates a watcher for them.
fn scan_workspace_folders(folders: &[String]) -> (Vec<FolderRoot>, Option<WorkspaceWatcher>) {
    let mut watcher = WorkspaceWatcher::new().ok();
    let mut tree = Vec::new();

    for folder_str in folders {
        let folder_path = PathBuf::from(folder_str);
        let entries = scan_dir_safe(&folder_path);
        try_watch_folder(&mut watcher, &folder_path);
        tree.push(FolderRoot {
            path: folder_path,
            entries,
            expanded: true,
        });
    }

    (tree, watcher)
}

/// Starts watching a folder if the watcher is available and the folder exists.
fn try_watch_folder(watcher: &mut Option<WorkspaceWatcher>, folder_path: &Path) {
    if let Some(ref mut w) = watcher {
        if folder_path.is_dir() {
            if let Err(e) = w.watch(folder_path) {
                tracing::warn!("Failed to watch {}: {e}", folder_path.display());
            }
        }
    }
}

/// Scans a directory if it exists, returning an empty list on failure or non-directory paths.
fn scan_dir_safe(folder_path: &Path) -> Vec<TreeEntry> {
    if folder_path.is_dir() {
        scan_directory(folder_path).unwrap_or_default()
    } else {
        Vec::new()
    }
}

impl App {
    /// Returns the cached workspace list, refreshing it from the DB if stale.
    pub(crate) fn get_cached_workspace_list(&mut self) -> &Vec<(String, String)> {
        if self.cached_workspace_list.is_none() {
            let list = self
                .workspace_store
                .as_ref()
                .and_then(|s| s.list_workspaces().ok())
                .unwrap_or_default()
                .into_iter()
                .map(|ws| (ws.id, ws.name))
                .collect();
            self.cached_workspace_list = Some(list);
        }
        self.cached_workspace_list.as_ref().unwrap()
    }

    /// Invalidates the cached workspace list, forcing a DB re-read next access.
    fn invalidate_workspace_cache(&mut self) {
        self.cached_workspace_list = None;
    }

    /// Initializes the workspace store. Called during App construction.
    pub(crate) fn init_workspace_store(portable: bool) -> Option<WorkspaceStore> {
        let path = if portable {
            rust_pad_config::paths::portable_workspace_file_path()
        } else {
            WorkspaceStore::workspace_path()
        };
        match WorkspaceStore::open(&path) {
            Ok(store) => Some(store),
            Err(e) => {
                let msg = format!("Failed to open workspace store: {e}");
                tracing::warn!("{msg}");
                crate::problem_log::log_problem(&msg);
                None
            }
        }
    }

    /// Activates a workspace in the sidebar (shared setup for create/open).
    fn activate_sidebar(
        &mut self,
        id: String,
        name: String,
        tree: Vec<FolderRoot>,
        watcher: Option<WorkspaceWatcher>,
    ) {
        self.workspace_sidebar.workspace_id = Some(id);
        self.workspace_sidebar.workspace_name = name;
        self.workspace_sidebar.tree = tree;
        self.workspace_sidebar.visible = true;
        self.workspace_sidebar.watcher = watcher;
        self.invalidate_workspace_cache();
    }

    /// Applies a filesystem event to the sidebar tree.
    fn notify_tree(&mut self, event: &FsEvent) {
        crate::workspace::scanner::apply_fs_event(&mut self.workspace_sidebar.tree, event);
    }

    /// Creates a new workspace with the given name and activates it.
    pub(crate) fn create_workspace(&mut self, name: &str) {
        let Some(store) = &self.workspace_store else {
            return;
        };

        let entry = WorkspaceEntry {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.to_string(),
            folders: Vec::new(),
            created_at: chrono::Utc::now().to_rfc3339(),
        };

        if let Err(e) = store.save_workspace(&entry) {
            let msg = format!("Failed to save new workspace: {e}");
            tracing::warn!("{msg}");
            crate::problem_log::log_problem(&msg);
            return;
        }

        if let Err(e) = store.set_active_workspace_id(Some(&entry.id)) {
            tracing::warn!("Failed to set active workspace: {e}");
        }

        self.activate_sidebar(
            entry.id,
            entry.name,
            Vec::new(),
            WorkspaceWatcher::new().ok(),
        );
    }

    /// Creates a new workspace with an auto-generated unique name.
    pub(crate) fn create_new_workspace(&mut self) {
        let existing = self
            .workspace_store
            .as_ref()
            .and_then(|s| s.list_workspaces().ok())
            .unwrap_or_default();
        let name = generate_workspace_name(&existing);
        self.create_workspace(&name); // invalidates cache internally
    }

    /// Opens an existing workspace by ID.
    pub(crate) fn open_workspace(&mut self, id: &str) {
        let Some(store) = &self.workspace_store else {
            return;
        };

        let entries = match store.list_workspaces() {
            Ok(e) => e,
            Err(e) => {
                tracing::warn!("Failed to list workspaces: {e}");
                return;
            }
        };

        let Some(entry) = entries.into_iter().find(|e| e.id == id) else {
            tracing::warn!("Workspace {id} not found");
            return;
        };

        if let Err(e) = store.set_active_workspace_id(Some(&entry.id)) {
            tracing::warn!("Failed to set active workspace: {e}");
        }

        let (tree, watcher) = scan_workspace_folders(&entry.folders);
        self.activate_sidebar(entry.id, entry.name, tree, watcher);
    }

    /// Closes the active workspace (hides sidebar, stops watcher).
    pub(crate) fn close_workspace(&mut self) {
        self.workspace_sidebar.visible = false;
        self.workspace_sidebar.tree.clear();
        self.workspace_sidebar.watcher = None;
        self.workspace_sidebar.workspace_id = None;
        self.workspace_sidebar.workspace_name.clear();

        if let Some(store) = &self.workspace_store {
            if let Err(e) = store.set_active_workspace_id(None) {
                tracing::warn!("Failed to clear active workspace: {e}");
            }
        }
    }

    /// Switches to a different workspace.
    pub(crate) fn switch_workspace(&mut self, id: &str) {
        self.close_workspace();
        self.open_workspace(id);
    }

    /// Adds a folder to the active workspace via a folder picker dialog.
    pub(crate) fn add_folder_to_workspace(&mut self) {
        let mut dialog = rfd::FileDialog::new();
        if let Some(start_dir) = self.file_dialog.resolve_directory() {
            dialog = dialog.set_directory(&start_dir);
        }
        let folder = dialog.pick_folder();

        let Some(folder_path) = folder else {
            return;
        };

        self.add_folder_path_to_workspace(&folder_path);
    }

    /// Adds a specific folder path to the active workspace.
    pub(crate) fn add_folder_path_to_workspace(&mut self, folder_path: &Path) {
        let Some(store) = &self.workspace_store else {
            return;
        };
        let Some(ws_id) = self.workspace_sidebar.workspace_id.clone() else {
            return;
        };

        if self.is_duplicate_or_nested_folder(folder_path) {
            let msg = format!(
                "Folder '{}' was not added: it duplicates or overlaps with an existing workspace folder.",
                folder_path.display()
            );
            tracing::info!("{msg}");
            crate::problem_log::log_problem(&msg);
            return;
        }

        // Update store
        let folder_str = folder_path.to_string_lossy().into_owned();
        let mut entries = store.list_workspaces().unwrap_or_default();
        if let Some(ws) = entries.iter_mut().find(|e| e.id == ws_id) {
            ws.folders.push(folder_str);
            if let Err(e) = store.save_workspace(ws) {
                let msg = format!("Failed to save workspace after adding folder: {e}");
                tracing::warn!("{msg}");
                crate::problem_log::log_problem(&msg);
                return;
            }
        }

        // Scan, watch, and add to tree
        let scanned = scan_dir_safe(folder_path);
        try_watch_folder(&mut self.workspace_sidebar.watcher, folder_path);
        self.workspace_sidebar.tree.push(FolderRoot {
            path: folder_path.to_path_buf(),
            entries: scanned,
            expanded: true,
        });
    }

    /// Checks if a folder path duplicates or overlaps with existing workspace folders.
    fn is_duplicate_or_nested_folder(&self, folder_path: &Path) -> bool {
        self.workspace_sidebar.tree.iter().any(|r| {
            r.path == folder_path
                || folder_path.starts_with(&r.path)
                || r.path.starts_with(folder_path)
        })
    }

    /// Removes a folder from the active workspace (not from disk).
    pub(crate) fn remove_folder_from_workspace(&mut self, path: &Path) {
        let Some(store) = &self.workspace_store else {
            return;
        };
        let Some(ws_id) = self.workspace_sidebar.workspace_id.clone() else {
            return;
        };

        // Stop watching
        if let Some(ref mut w) = self.workspace_sidebar.watcher {
            let _ = w.unwatch(path);
        }

        // Remove from tree
        self.workspace_sidebar.tree.retain(|r| r.path != path);

        // Update store
        let folder_str = path.to_string_lossy().into_owned();
        let mut entries = store.list_workspaces().unwrap_or_default();
        if let Some(ws) = entries.iter_mut().find(|e| e.id == ws_id) {
            ws.folders.retain(|f| f != &folder_str);
            if let Err(e) = store.save_workspace(ws) {
                tracing::warn!("Failed to save workspace after removing folder: {e}");
            }
        }
    }

    /// Renames the active workspace.
    pub(crate) fn rename_workspace(&mut self, id: &str, new_name: &str) {
        let Some(store) = &self.workspace_store else {
            return;
        };

        let mut entries = store.list_workspaces().unwrap_or_default();
        if let Some(ws) = entries.iter_mut().find(|e| e.id == id) {
            ws.name = new_name.to_string();
            if let Err(e) = store.save_workspace(ws) {
                tracing::warn!("Failed to rename workspace: {e}");
            }
        }

        if self.workspace_sidebar.workspace_id.as_deref() == Some(id) {
            self.workspace_sidebar.workspace_name = new_name.to_string();
        }
        self.invalidate_workspace_cache();
    }

    /// Deletes a workspace from the store. Closes it if active.
    pub(crate) fn delete_workspace(&mut self, id: &str) {
        if self.workspace_sidebar.workspace_id.as_deref() == Some(id) {
            self.close_workspace();
        }

        if let Some(store) = &self.workspace_store {
            if let Err(e) = store.delete_workspace(id) {
                tracing::warn!("Failed to delete workspace: {e}");
            }
        }
        self.invalidate_workspace_cache();
    }

    /// Creates a new empty file with the given name in the specified directory.
    pub(crate) fn create_new_file_in_workspace(&mut self, parent: &Path, name: &str) {
        let path = parent.join(name);
        if path.exists() {
            let msg = format!("'{}' already exists in '{}'", name, parent.display());
            tracing::warn!("{msg}");
            crate::problem_log::log_problem(&msg);
            return;
        }
        if let Err(e) = std::fs::write(&path, "") {
            let msg = format!("Failed to create file '{}': {e}", path.display());
            tracing::warn!("{msg}");
            crate::problem_log::log_problem(&msg);
            return;
        }
        self.notify_tree(&FsEvent::Created(path));
    }

    /// Creates a new subdirectory with the given name in the specified directory.
    pub(crate) fn create_new_folder_in_workspace(&mut self, parent: &Path, name: &str) {
        let path = parent.join(name);
        if path.exists() {
            let msg = format!("'{}' already exists in '{}'", name, parent.display());
            tracing::warn!("{msg}");
            crate::problem_log::log_problem(&msg);
            return;
        }
        if let Err(e) = std::fs::create_dir(&path) {
            let msg = format!("Failed to create folder '{}': {e}", path.display());
            tracing::warn!("{msg}");
            crate::problem_log::log_problem(&msg);
            return;
        }
        self.notify_tree(&FsEvent::Created(path));
    }

    /// Renames a file or folder in the workspace.
    pub(crate) fn rename_entry_in_workspace(&mut self, old_path: &Path, new_name: &str) {
        let Some(parent) = old_path.parent() else {
            return;
        };
        let new_path = parent.join(new_name);

        if new_path.exists() {
            let msg = format!("'{}' already exists", new_path.display());
            tracing::warn!("{msg}");
            crate::problem_log::log_problem(&msg);
            return;
        }

        if let Err(e) = std::fs::rename(old_path, &new_path) {
            let msg = format!(
                "Failed to rename '{}' to '{}': {e}",
                old_path.display(),
                new_path.display()
            );
            tracing::warn!("{msg}");
            crate::problem_log::log_problem(&msg);
            return;
        }

        self.notify_tree(&FsEvent::Removed(old_path.to_path_buf()));
        self.notify_tree(&FsEvent::Created(new_path));
    }

    /// Processes a sidebar action returned from the sidebar render pass.
    pub(crate) fn handle_sidebar_action(&mut self, action: SidebarAction) {
        match action {
            SidebarAction::OpenFile(path) => {
                self.open_file_path(&path);
            }
            SidebarAction::DeleteFile(path) => {
                if let Err(e) = trash::delete(&path) {
                    let msg = format!("Failed to delete '{}': {e}", path.display());
                    tracing::warn!("{msg}");
                    crate::problem_log::log_problem(&msg);
                }
            }
            SidebarAction::AddFolder => {
                self.add_folder_to_workspace();
            }
            SidebarAction::RemoveFolder(path) => {
                self.remove_folder_from_workspace(&path);
            }
            SidebarAction::SwitchWorkspace(id) => {
                self.switch_workspace(&id);
            }
            SidebarAction::CloseWorkspace => {
                self.close_workspace();
            }
            SidebarAction::CreateWorkspace => {
                self.create_new_workspace();
            }
            SidebarAction::RenameWorkspace(id, new_name) => {
                self.rename_workspace(&id, &new_name);
            }
            SidebarAction::DeleteWorkspace(id) => {
                self.delete_workspace(&id);
            }
            SidebarAction::ConfirmNewFile(parent, name) => {
                self.create_new_file_in_workspace(&parent, &name);
            }
            SidebarAction::ConfirmNewFolder(parent, name) => {
                self.create_new_folder_in_workspace(&parent, &name);
            }
            SidebarAction::ConfirmRenameEntry(old_path, new_name) => {
                self.rename_entry_in_workspace(&old_path, &new_name);
            }
            SidebarAction::None => {}
        }
    }

    /// Polls filesystem events and applies them to the sidebar tree.
    pub(crate) fn tick_workspace_watcher(&mut self) {
        let events = self
            .workspace_sidebar
            .watcher
            .as_ref()
            .map(|w| w.poll_events())
            .unwrap_or_default();
        for event in &events {
            self.notify_tree(event);
        }
    }

    /// Restores the workspace on startup if one was active.
    pub(crate) fn restore_workspace_on_startup(&mut self) {
        let Some(store) = &self.workspace_store else {
            return;
        };
        let active_id = match store.get_active_workspace_id() {
            Ok(Some(id)) => id,
            _ => return,
        };
        self.open_workspace(&active_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_entry(name: &str) -> WorkspaceEntry {
        WorkspaceEntry {
            id: format!("id-{name}"),
            name: name.to_string(),
            folders: Vec::new(),
            created_at: String::new(),
        }
    }

    #[test]
    fn test_generate_workspace_name_empty_list() {
        let name = generate_workspace_name(&[]);
        assert_eq!(name, "New Workspace");
    }

    #[test]
    fn test_generate_workspace_name_no_conflict() {
        let existing = vec![make_entry("My Project")];
        let name = generate_workspace_name(&existing);
        assert_eq!(name, "New Workspace");
    }

    #[test]
    fn test_generate_workspace_name_first_conflict() {
        let existing = vec![make_entry("New Workspace")];
        let name = generate_workspace_name(&existing);
        assert_eq!(name, "New Workspace 2");
    }

    #[test]
    fn test_generate_workspace_name_multiple_conflicts() {
        let existing = vec![
            make_entry("New Workspace"),
            make_entry("New Workspace 2"),
            make_entry("New Workspace 3"),
        ];
        let name = generate_workspace_name(&existing);
        assert_eq!(name, "New Workspace 4");
    }

    #[test]
    fn test_generate_workspace_name_gap_in_numbering() {
        let existing = vec![make_entry("New Workspace"), make_entry("New Workspace 3")];
        let name = generate_workspace_name(&existing);
        assert_eq!(name, "New Workspace 2");
    }

    #[test]
    fn test_generate_unique_name_no_conflict() {
        let dir = tempfile::tempdir().unwrap();
        let name = generate_unique_name(dir.path(), "new_file.txt", false);
        assert_eq!(name, "new_file.txt");
    }

    #[test]
    fn test_generate_unique_name_file_conflict() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("new_file.txt"), "").unwrap();
        let name = generate_unique_name(dir.path(), "new_file.txt", false);
        assert_eq!(name, "new_file 2.txt");
    }

    #[test]
    fn test_generate_unique_name_multiple_file_conflicts() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("new_file.txt"), "").unwrap();
        std::fs::write(dir.path().join("new_file 2.txt"), "").unwrap();
        std::fs::write(dir.path().join("new_file 3.txt"), "").unwrap();
        let name = generate_unique_name(dir.path(), "new_file.txt", false);
        assert_eq!(name, "new_file 4.txt");
    }

    #[test]
    fn test_generate_unique_name_dir_no_conflict() {
        let dir = tempfile::tempdir().unwrap();
        let name = generate_unique_name(dir.path(), "new_folder", true);
        assert_eq!(name, "new_folder");
    }

    #[test]
    fn test_generate_unique_name_dir_conflict() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("new_folder")).unwrap();
        let name = generate_unique_name(dir.path(), "new_folder", true);
        assert_eq!(name, "new_folder 2");
    }

    #[test]
    fn test_generate_unique_name_dir_multiple_conflicts() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("new_folder")).unwrap();
        std::fs::create_dir(dir.path().join("new_folder 2")).unwrap();
        let name = generate_unique_name(dir.path(), "new_folder", true);
        assert_eq!(name, "new_folder 3");
    }

    #[test]
    fn test_generate_unique_name_no_extension() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("README"), "").unwrap();
        let name = generate_unique_name(dir.path(), "README", false);
        assert_eq!(name, "README 2");
    }

    #[test]
    fn test_scan_dir_safe_nonexistent() {
        let result = scan_dir_safe(Path::new("/nonexistent_dir_xyz_123"));
        assert!(result.is_empty());
    }

    #[test]
    fn test_scan_dir_safe_valid_directory() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("file.txt"), "").unwrap();
        let result = scan_dir_safe(dir.path());
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "file.txt");
    }

    #[test]
    fn test_scan_dir_safe_file_path_returns_empty() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("not_a_dir.txt");
        std::fs::write(&file, "").unwrap();
        let result = scan_dir_safe(&file);
        assert!(result.is_empty());
    }

    #[test]
    fn test_scan_workspace_folders_empty() {
        let (tree, watcher) = scan_workspace_folders(&[]);
        assert!(tree.is_empty());
        // Watcher is created but has nothing to watch
        assert!(watcher.is_some());
    }

    #[test]
    fn test_scan_workspace_folders_with_real_dirs() {
        let dir1 = tempfile::tempdir().unwrap();
        let dir2 = tempfile::tempdir().unwrap();
        std::fs::write(dir1.path().join("a.txt"), "").unwrap();
        std::fs::write(dir2.path().join("b.txt"), "").unwrap();
        std::fs::create_dir(dir2.path().join("subdir")).unwrap();

        let folders = vec![
            dir1.path().to_string_lossy().into_owned(),
            dir2.path().to_string_lossy().into_owned(),
        ];
        let (tree, watcher) = scan_workspace_folders(&folders);

        assert_eq!(tree.len(), 2);
        assert_eq!(tree[0].entries.len(), 1);
        assert_eq!(tree[0].entries[0].name, "a.txt");
        // dir2 has a file and a directory
        assert_eq!(tree[1].entries.len(), 2);
        assert!(tree[0].expanded);
        assert!(tree[1].expanded);
        assert!(watcher.is_some());
    }

    #[test]
    fn test_scan_workspace_folders_with_nonexistent() {
        let folders = vec!["/nonexistent_dir_scan_test_xyz".to_string()];
        let (tree, _watcher) = scan_workspace_folders(&folders);

        assert_eq!(tree.len(), 1);
        assert!(
            tree[0].entries.is_empty(),
            "Nonexistent dir yields empty entries"
        );
        assert_eq!(
            tree[0].path,
            PathBuf::from("/nonexistent_dir_scan_test_xyz")
        );
    }

    #[test]
    fn test_try_watch_folder_with_none_watcher() {
        let dir = tempfile::tempdir().unwrap();
        let mut watcher: Option<WorkspaceWatcher> = None;
        // Should not panic — no-op when watcher is None
        try_watch_folder(&mut watcher, dir.path());
        assert!(watcher.is_none());
    }

    #[test]
    fn test_try_watch_folder_with_valid_watcher() {
        let dir = tempfile::tempdir().unwrap();
        let mut watcher = Some(WorkspaceWatcher::new().expect("create watcher"));
        try_watch_folder(&mut watcher, dir.path());
        // Watcher should still be Some (not consumed or invalidated)
        assert!(watcher.is_some());
    }

    #[test]
    fn test_try_watch_folder_nonexistent_dir() {
        let mut watcher = Some(WorkspaceWatcher::new().expect("create watcher"));
        // Nonexistent path is not a dir, so watch should be skipped silently
        try_watch_folder(&mut watcher, Path::new("/nonexistent_xyz_watch_test"));
        assert!(watcher.is_some());
    }
}
