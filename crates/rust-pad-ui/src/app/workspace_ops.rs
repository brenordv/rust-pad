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
use crate::workspace::tree::FolderRoot;
use crate::workspace::watcher::WorkspaceWatcher;

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

impl App {
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

        self.workspace_sidebar.workspace_id = Some(entry.id);
        self.workspace_sidebar.workspace_name = entry.name;
        self.workspace_sidebar.tree.clear();
        self.workspace_sidebar.visible = true;

        // Start a fresh watcher
        self.workspace_sidebar.watcher = WorkspaceWatcher::new().ok();
    }

    /// Creates a new workspace with an auto-generated unique name.
    pub(crate) fn create_new_workspace(&mut self) {
        let existing = self
            .workspace_store
            .as_ref()
            .and_then(|s| s.list_workspaces().ok())
            .unwrap_or_default();
        let name = generate_workspace_name(&existing);
        self.create_workspace(&name);
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

        self.workspace_sidebar.workspace_id = Some(entry.id);
        self.workspace_sidebar.workspace_name = entry.name;
        self.workspace_sidebar.visible = true;

        // Scan folders and start watching
        let mut watcher = WorkspaceWatcher::new().ok();
        let mut tree = Vec::new();

        for folder_str in &entry.folders {
            let folder_path = PathBuf::from(folder_str);
            let entries = if folder_path.is_dir() {
                scan_directory(&folder_path).unwrap_or_default()
            } else {
                Vec::new()
            };

            if let Some(ref mut w) = watcher {
                if folder_path.is_dir() {
                    if let Err(e) = w.watch(&folder_path) {
                        tracing::warn!("Failed to watch {}: {e}", folder_path.display());
                    }
                }
            }

            tree.push(FolderRoot {
                path: folder_path,
                entries,
                expanded: true,
            });
        }

        self.workspace_sidebar.tree = tree;
        self.workspace_sidebar.watcher = watcher;
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

        // Prevent duplicate folders, sub-folders of existing roots, or parent
        // folders when a subfolder is already in the workspace.
        let folder_str = folder_path.to_string_lossy().into_owned();
        let is_duplicate_or_nested = self.workspace_sidebar.tree.iter().any(|r| {
            r.path == folder_path
                || folder_path.starts_with(&r.path)
                || r.path.starts_with(folder_path)
        });
        if is_duplicate_or_nested {
            let msg = format!(
                "Folder '{}' was not added: it duplicates or overlaps with an existing workspace folder.",
                folder_path.display()
            );
            tracing::info!("{msg}");
            crate::problem_log::log_problem(&msg);
            return;
        }

        // Update store
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

        // Scan and add to tree
        let scanned = if folder_path.is_dir() {
            scan_directory(folder_path).unwrap_or_default()
        } else {
            Vec::new()
        };

        // Start watching
        if let Some(ref mut w) = self.workspace_sidebar.watcher {
            if folder_path.is_dir() {
                if let Err(e) = w.watch(folder_path) {
                    tracing::warn!("Failed to watch {}: {e}", folder_path.display());
                }
            }
        }

        self.workspace_sidebar.tree.push(FolderRoot {
            path: folder_path.to_path_buf(),
            entries: scanned,
            expanded: true,
        });
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
            SidebarAction::None => {}
        }
    }

    /// Polls filesystem events and applies them to the sidebar tree.
    pub(crate) fn tick_workspace_watcher(&mut self) {
        if let Some(ref watcher) = self.workspace_sidebar.watcher {
            let events = watcher.poll_events();
            if !events.is_empty() {
                for event in &events {
                    crate::workspace::scanner::apply_fs_event(
                        &mut self.workspace_sidebar.tree,
                        event,
                    );
                }
            }
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
}
