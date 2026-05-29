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
fn scan_workspace_folders(
    folders: &[String],
    show_hidden: bool,
) -> (Vec<FolderRoot>, Option<WorkspaceWatcher>) {
    let mut watcher = WorkspaceWatcher::new().ok();
    let mut tree = Vec::new();

    for folder_str in folders {
        let folder_path = PathBuf::from(folder_str);
        let entries = scan_dir_safe(&folder_path, show_hidden);
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
fn scan_dir_safe(folder_path: &Path, show_hidden: bool) -> Vec<TreeEntry> {
    if folder_path.is_dir() {
        scan_directory(folder_path, show_hidden).unwrap_or_default()
    } else {
        Vec::new()
    }
}

/// Returns true if `name` is a simple, single-segment file or folder name
/// safe to pass to `parent.join(name)` without enabling path traversal.
///
/// Rejects: empty input, the special `.` / `..` segments, path separators
/// (`/` and `\`), embedded NUL, absolute Unix paths, and Windows drive
/// prefixes (`X:` form). Control characters (< 0x20) are also rejected
/// to keep workspace tree labels well-formed.
pub(crate) fn is_valid_simple_name(name: &str) -> bool {
    if name.is_empty() || name == "." || name == ".." {
        return false;
    }
    if name.starts_with('/') {
        return false;
    }
    // Windows drive prefix like "C:" or "C:\\..."
    let mut chars = name.chars();
    if let (Some(first), Some(second)) = (chars.next(), chars.next()) {
        if first.is_ascii_alphabetic() && second == ':' {
            return false;
        }
    }
    for c in name.chars() {
        if c == '/' || c == '\\' || c == '\0' || (c as u32) < 0x20 {
            return false;
        }
    }
    true
}

/// Maximum depth walked when checking for a symlink loop.
const SYMLINK_LOOP_DEPTH: usize = 64;

/// Maximum filesystem events applied to the sidebar tree per frame.
///
/// Bounds work per tick so a watcher event storm cannot starve the UI.
/// Surplus events stay queued and are drained on subsequent ticks.
const MAX_WATCHER_EVENTS_PER_TICK: usize = 1000;

/// Minimum interval between overflow warnings in the log.
const WATCHER_OVERFLOW_LOG_INTERVAL: std::time::Duration = std::time::Duration::from_secs(60);

/// Detects whether `folder_path` contains a symlink loop within the first
/// `SYMLINK_LOOP_DEPTH` directories reached by a depth-first walk.
///
/// Returns true if the same canonical path is observed twice during the walk
/// — the classic signature of a symlink cycle. Best-effort: a return value
/// of `false` does not guarantee the absence of a loop deeper in the tree,
/// but catches the foot-gun configurations users are likely to create.
fn has_symlink_loop(folder_path: &Path) -> bool {
    let mut seen: std::collections::HashSet<std::path::PathBuf> = std::collections::HashSet::new();
    let mut stack: Vec<(std::path::PathBuf, usize)> = vec![(folder_path.to_path_buf(), 0)];

    while let Some((path, depth)) = stack.pop() {
        if depth > SYMLINK_LOOP_DEPTH {
            continue;
        }
        let canon = match std::fs::canonicalize(&path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        if !seen.insert(canon.clone()) {
            return true;
        }
        let read_dir = match std::fs::read_dir(&path) {
            Ok(rd) => rd,
            Err(_) => continue,
        };
        for entry in read_dir.flatten() {
            let entry_path = entry.path();
            let file_type = match entry.file_type() {
                Ok(ft) => ft,
                Err(_) => continue,
            };
            if file_type.is_dir() || file_type.is_symlink() {
                stack.push((entry_path, depth + 1));
            }
        }
    }
    false
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
        crate::workspace::scanner::apply_fs_event(
            &mut self.workspace_sidebar.tree,
            event,
            self.workspace_sidebar.show_hidden,
        );
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

        let (tree, watcher) =
            scan_workspace_folders(&entry.folders, self.workspace_sidebar.show_hidden);
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

        if self.is_duplicate_folder(folder_path) {
            let msg = format!(
                "Folder '{}' was not added: it is already in the workspace.",
                folder_path.display()
            );
            tracing::info!("{msg}");
            crate::problem_log::log_problem(&msg);
            return;
        }

        if has_symlink_loop(folder_path) {
            let msg = format!(
                "Folder '{}' was not added: symlink loop detected.",
                folder_path.display()
            );
            tracing::warn!("{msg}");
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
        let scanned = scan_dir_safe(folder_path, self.workspace_sidebar.show_hidden);
        try_watch_folder(&mut self.workspace_sidebar.watcher, folder_path);
        self.workspace_sidebar.tree.push(FolderRoot {
            path: folder_path.to_path_buf(),
            entries: scanned,
            expanded: true,
        });
    }

    /// Checks if a folder path exactly matches an existing workspace root.
    ///
    /// Overlapping roots (nested or parent of an existing root) are allowed —
    /// the watcher deduplicates events and the tree displays each root
    /// independently. Only exact path equality is rejected.
    fn is_duplicate_folder(&self, folder_path: &Path) -> bool {
        self.workspace_sidebar
            .tree
            .iter()
            .any(|r| r.path == folder_path)
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
        if !is_valid_simple_name(name) {
            let msg =
                format!("Name '{name}' rejected: contains invalid characters or path separators");
            tracing::warn!("{msg}");
            crate::problem_log::log_problem(&msg);
            return;
        }
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
        if !is_valid_simple_name(name) {
            let msg =
                format!("Name '{name}' rejected: contains invalid characters or path separators");
            tracing::warn!("{msg}");
            crate::problem_log::log_problem(&msg);
            return;
        }
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
        if !is_valid_simple_name(new_name) {
            let msg = format!(
                "Name '{new_name}' rejected: contains invalid characters or path separators"
            );
            tracing::warn!("{msg}");
            crate::problem_log::log_problem(&msg);
            return;
        }
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
            SidebarAction::ToggleHiddenFiles => {
                self.toggle_hidden_files();
            }
            SidebarAction::ExpandAll => {
                self.workspace_sidebar.pending_bulk_collapse = Some(true);
            }
            SidebarAction::CollapseAll => {
                self.workspace_sidebar.pending_bulk_collapse = Some(false);
            }
            SidebarAction::None => {}
        }
    }

    /// Polls filesystem events and applies them to the sidebar tree.
    ///
    /// Caps work to [`MAX_WATCHER_EVENTS_PER_TICK`] events per call to keep
    /// the UI responsive during event storms (overlapping watchers,
    /// large refactors). Excess events stay queued for the next tick.
    pub(crate) fn tick_workspace_watcher(&mut self) {
        let mut events = self
            .workspace_sidebar
            .watcher
            .as_ref()
            .map(|w| w.poll_events())
            .unwrap_or_default();

        let overflowed = events.len() > MAX_WATCHER_EVENTS_PER_TICK;
        if overflowed {
            let dropped = events.len() - MAX_WATCHER_EVENTS_PER_TICK;
            events.truncate(MAX_WATCHER_EVENTS_PER_TICK);
            let now = std::time::Instant::now();
            let should_log = self
                .last_watcher_overflow_log
                .is_none_or(|t| now.duration_since(t) > WATCHER_OVERFLOW_LOG_INTERVAL);
            if should_log {
                tracing::warn!(
                    "Watcher tick overflowed: {dropped} events dropped (cap={MAX_WATCHER_EVENTS_PER_TICK})"
                );
                self.last_watcher_overflow_log = Some(now);
            }
        }

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

    /// Toggles hidden file visibility and re-scans all folder roots.
    pub(crate) fn toggle_hidden_files(&mut self) {
        self.workspace_sidebar.show_hidden = !self.workspace_sidebar.show_hidden;
        self.rescan_workspace_tree();
    }

    /// Re-scans all folder roots using the current `show_hidden` setting.
    pub(crate) fn rescan_workspace_tree(&mut self) {
        for root in &mut self.workspace_sidebar.tree {
            root.entries = scan_dir_safe(&root.path, self.workspace_sidebar.show_hidden);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::app_with_workspace;
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
        let result = scan_dir_safe(Path::new("/nonexistent_dir_xyz_123"), false);
        assert!(result.is_empty());
    }

    #[test]
    fn test_scan_dir_safe_valid_directory() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("file.txt"), "").unwrap();
        let result = scan_dir_safe(dir.path(), false);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "file.txt");
    }

    #[test]
    fn test_scan_dir_safe_file_path_returns_empty() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("not_a_dir.txt");
        std::fs::write(&file, "").unwrap();
        let result = scan_dir_safe(&file, false);
        assert!(result.is_empty());
    }

    #[test]
    fn test_scan_workspace_folders_empty() {
        let (tree, watcher) = scan_workspace_folders(&[], false);
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
        let (tree, watcher) = scan_workspace_folders(&folders, false);

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
        let (tree, _watcher) = scan_workspace_folders(&folders, false);

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

    // ── App-level integration tests ──────────────────────────────────

    #[test]
    fn test_app_create_workspace() {
        let (mut app, _dir) = app_with_workspace();
        app.create_workspace("Test WS");

        assert!(app.workspace_sidebar.visible);
        assert_eq!(app.workspace_sidebar.workspace_name, "Test WS");
        assert!(app.workspace_sidebar.workspace_id.is_some());
        assert!(app.workspace_sidebar.tree.is_empty());
    }

    #[test]
    fn test_app_create_new_workspace_generates_unique_name() {
        let (mut app, _dir) = app_with_workspace();
        app.create_new_workspace();
        assert_eq!(app.workspace_sidebar.workspace_name, "New Workspace");

        // Creating another should increment the name
        app.create_new_workspace();
        assert_eq!(app.workspace_sidebar.workspace_name, "New Workspace 2");
    }

    #[test]
    fn test_app_close_workspace() {
        let (mut app, _dir) = app_with_workspace();
        app.create_workspace("To Close");
        assert!(app.workspace_sidebar.visible);

        app.close_workspace();
        assert!(!app.workspace_sidebar.visible);
        assert!(app.workspace_sidebar.workspace_id.is_none());
        assert!(app.workspace_sidebar.workspace_name.is_empty());
        assert!(app.workspace_sidebar.tree.is_empty());
        assert!(app.workspace_sidebar.watcher.is_none());
    }

    #[test]
    fn test_app_open_workspace() {
        let (mut app, _dir) = app_with_workspace();
        app.create_workspace("Open Me");
        let ws_id = app.workspace_sidebar.workspace_id.clone().unwrap();
        app.close_workspace();

        app.open_workspace(&ws_id);
        assert!(app.workspace_sidebar.visible);
        assert_eq!(app.workspace_sidebar.workspace_name, "Open Me");
        assert_eq!(
            app.workspace_sidebar.workspace_id.as_deref(),
            Some(ws_id.as_str())
        );
    }

    #[test]
    fn test_app_open_nonexistent_workspace() {
        let (mut app, _dir) = app_with_workspace();
        app.open_workspace("nonexistent-id");
        // Should remain closed
        assert!(!app.workspace_sidebar.visible);
        assert!(app.workspace_sidebar.workspace_id.is_none());
    }

    #[test]
    fn test_app_switch_workspace() {
        let (mut app, _dir) = app_with_workspace();
        app.create_workspace("First");
        let first_id = app.workspace_sidebar.workspace_id.clone().unwrap();

        app.create_workspace("Second");
        let second_id = app.workspace_sidebar.workspace_id.clone().unwrap();
        assert_eq!(app.workspace_sidebar.workspace_name, "Second");

        app.switch_workspace(&first_id);
        assert_eq!(app.workspace_sidebar.workspace_name, "First");
        assert_eq!(
            app.workspace_sidebar.workspace_id.as_deref(),
            Some(first_id.as_str())
        );

        app.switch_workspace(&second_id);
        assert_eq!(app.workspace_sidebar.workspace_name, "Second");
    }

    #[test]
    fn test_app_rename_workspace() {
        let (mut app, _dir) = app_with_workspace();
        app.create_workspace("Before");
        let ws_id = app.workspace_sidebar.workspace_id.clone().unwrap();

        app.rename_workspace(&ws_id, "After");
        assert_eq!(app.workspace_sidebar.workspace_name, "After");

        // Verify persisted in store
        let entries = app
            .workspace_store
            .as_ref()
            .unwrap()
            .list_workspaces()
            .unwrap();
        assert_eq!(entries[0].name, "After");
    }

    #[test]
    fn test_app_rename_workspace_invalidates_cache() {
        let (mut app, _dir) = app_with_workspace();
        app.create_workspace("Cached Name");
        let ws_id = app.workspace_sidebar.workspace_id.clone().unwrap();

        // Populate cache
        let _ = app.get_cached_workspace_list();
        assert!(app.cached_workspace_list.is_some());

        app.rename_workspace(&ws_id, "New Name");
        // Cache should be invalidated
        assert!(app.cached_workspace_list.is_none());
    }

    #[test]
    fn test_app_delete_workspace() {
        let (mut app, _dir) = app_with_workspace();
        app.create_workspace("To Delete");
        let ws_id = app.workspace_sidebar.workspace_id.clone().unwrap();

        app.delete_workspace(&ws_id);
        // Active workspace was deleted, so sidebar should be closed
        assert!(!app.workspace_sidebar.visible);
        assert!(app.workspace_sidebar.workspace_id.is_none());

        let entries = app
            .workspace_store
            .as_ref()
            .unwrap()
            .list_workspaces()
            .unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn test_app_delete_inactive_workspace() {
        let (mut app, _dir) = app_with_workspace();
        app.create_workspace("Keep");
        app.create_workspace("Delete Me");
        let delete_id = app.workspace_sidebar.workspace_id.clone().unwrap();

        // Switch to the first one
        let entries = app
            .workspace_store
            .as_ref()
            .unwrap()
            .list_workspaces()
            .unwrap();
        let keep_id = entries
            .iter()
            .find(|e| e.name == "Keep")
            .unwrap()
            .id
            .clone();
        app.switch_workspace(&keep_id);

        app.delete_workspace(&delete_id);
        // Active workspace should still be open
        assert!(app.workspace_sidebar.visible);
        assert_eq!(app.workspace_sidebar.workspace_name, "Keep");

        let entries = app
            .workspace_store
            .as_ref()
            .unwrap()
            .list_workspaces()
            .unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "Keep");
    }

    #[test]
    fn test_app_add_folder_path_to_workspace() {
        let (mut app, _dir) = app_with_workspace();
        let folder = tempfile::tempdir().unwrap();
        std::fs::write(folder.path().join("test.rs"), "fn main() {}").unwrap();

        app.create_workspace("With Folder");
        app.add_folder_path_to_workspace(folder.path());

        assert_eq!(app.workspace_sidebar.tree.len(), 1);
        assert_eq!(app.workspace_sidebar.tree[0].path, folder.path());
        assert!(!app.workspace_sidebar.tree[0].entries.is_empty());
    }

    #[test]
    fn test_app_add_duplicate_folder_rejected() {
        let (mut app, _dir) = app_with_workspace();
        let folder = tempfile::tempdir().unwrap();

        app.create_workspace("Dup Test");
        app.add_folder_path_to_workspace(folder.path());
        assert_eq!(app.workspace_sidebar.tree.len(), 1);

        // Adding the same folder again should be rejected
        app.add_folder_path_to_workspace(folder.path());
        assert_eq!(app.workspace_sidebar.tree.len(), 1);
    }

    #[test]
    fn test_app_add_nested_folder_allowed() {
        let (mut app, _dir) = app_with_workspace();
        let parent = tempfile::tempdir().unwrap();
        let child = parent.path().join("child");
        std::fs::create_dir(&child).unwrap();

        app.create_workspace("Nested Test");
        app.add_folder_path_to_workspace(parent.path());
        assert_eq!(app.workspace_sidebar.tree.len(), 1);

        // Overlapping (nested) folders are allowed — only exact match is rejected.
        app.add_folder_path_to_workspace(&child);
        assert_eq!(app.workspace_sidebar.tree.len(), 2);
    }

    #[test]
    fn test_app_add_parent_folder_of_existing_allowed() {
        let (mut app, _dir) = app_with_workspace();
        let parent = tempfile::tempdir().unwrap();
        let child = parent.path().join("child");
        std::fs::create_dir(&child).unwrap();

        app.create_workspace("Parent Test");
        app.add_folder_path_to_workspace(&child);
        assert_eq!(app.workspace_sidebar.tree.len(), 1);

        // Adding a parent that overlaps with an existing root is allowed.
        app.add_folder_path_to_workspace(parent.path());
        assert_eq!(app.workspace_sidebar.tree.len(), 2);
    }

    #[test]
    fn test_app_remove_folder_from_workspace() {
        let (mut app, _dir) = app_with_workspace();
        let folder = tempfile::tempdir().unwrap();

        app.create_workspace("Remove Test");
        app.add_folder_path_to_workspace(folder.path());
        assert_eq!(app.workspace_sidebar.tree.len(), 1);

        app.remove_folder_from_workspace(folder.path());
        assert!(app.workspace_sidebar.tree.is_empty());

        // Verify persisted
        let entries = app
            .workspace_store
            .as_ref()
            .unwrap()
            .list_workspaces()
            .unwrap();
        let ws = entries.iter().find(|e| e.name == "Remove Test").unwrap();
        assert!(ws.folders.is_empty());
    }

    #[test]
    fn test_app_create_new_file_in_workspace() {
        let (mut app, _dir) = app_with_workspace();
        let folder = tempfile::tempdir().unwrap();

        app.create_workspace("File Create");
        app.add_folder_path_to_workspace(folder.path());

        app.create_new_file_in_workspace(folder.path(), "hello.txt");
        assert!(folder.path().join("hello.txt").exists());
        // Tree should be updated with the new file
        let has_file = app.workspace_sidebar.tree[0]
            .entries
            .iter()
            .any(|e| e.name == "hello.txt");
        assert!(has_file, "New file should appear in tree");
    }

    #[test]
    fn test_app_create_file_already_exists() {
        let (mut app, _dir) = app_with_workspace();
        let folder = tempfile::tempdir().unwrap();
        std::fs::write(folder.path().join("exists.txt"), "original").unwrap();

        app.create_workspace("Exists Test");
        app.add_folder_path_to_workspace(folder.path());

        app.create_new_file_in_workspace(folder.path(), "exists.txt");
        // File content should not be overwritten
        let content = std::fs::read_to_string(folder.path().join("exists.txt")).unwrap();
        assert_eq!(content, "original");
    }

    #[test]
    fn test_app_create_new_folder_in_workspace() {
        let (mut app, _dir) = app_with_workspace();
        let folder = tempfile::tempdir().unwrap();

        app.create_workspace("Folder Create");
        app.add_folder_path_to_workspace(folder.path());

        app.create_new_folder_in_workspace(folder.path(), "subdir");
        assert!(folder.path().join("subdir").is_dir());
        let has_dir = app.workspace_sidebar.tree[0]
            .entries
            .iter()
            .any(|e| e.name == "subdir");
        assert!(has_dir, "New folder should appear in tree");
    }

    #[test]
    fn test_app_create_folder_already_exists() {
        let (mut app, _dir) = app_with_workspace();
        let folder = tempfile::tempdir().unwrap();
        std::fs::create_dir(folder.path().join("existing")).unwrap();
        std::fs::write(folder.path().join("existing").join("keep.txt"), "keep").unwrap();

        app.create_workspace("Exists Folder");
        app.add_folder_path_to_workspace(folder.path());

        // Should not overwrite existing directory
        app.create_new_folder_in_workspace(folder.path(), "existing");
        assert!(folder.path().join("existing").join("keep.txt").exists());
    }

    #[test]
    fn test_app_rename_entry_in_workspace() {
        let (mut app, _dir) = app_with_workspace();
        let folder = tempfile::tempdir().unwrap();
        let old_file = folder.path().join("old_name.rs");
        std::fs::write(&old_file, "fn main() {}").unwrap();

        app.create_workspace("Rename Entry");
        app.add_folder_path_to_workspace(folder.path());

        app.rename_entry_in_workspace(&old_file, "new_name.rs");
        assert!(!old_file.exists());
        assert!(folder.path().join("new_name.rs").exists());
    }

    #[test]
    fn test_app_rename_entry_target_exists() {
        let (mut app, _dir) = app_with_workspace();
        let folder = tempfile::tempdir().unwrap();
        std::fs::write(folder.path().join("a.rs"), "a").unwrap();
        std::fs::write(folder.path().join("b.rs"), "b").unwrap();

        app.create_workspace("Rename Conflict");
        app.add_folder_path_to_workspace(folder.path());

        // Renaming a.rs to b.rs should fail (b.rs already exists)
        app.rename_entry_in_workspace(&folder.path().join("a.rs"), "b.rs");
        // Both files should still exist with original content
        assert_eq!(
            std::fs::read_to_string(folder.path().join("a.rs")).unwrap(),
            "a"
        );
        assert_eq!(
            std::fs::read_to_string(folder.path().join("b.rs")).unwrap(),
            "b"
        );
    }

    #[test]
    fn test_app_rename_entry_no_parent() {
        let (mut app, _dir) = app_with_workspace();
        // Renaming a root path with no parent should be a no-op
        #[cfg(unix)]
        let root = Path::new("/");
        #[cfg(windows)]
        let root = Path::new("C:\\");
        // This should just return without panicking
        app.rename_entry_in_workspace(root, "new_name");
    }

    #[test]
    fn test_app_handle_sidebar_action_close() {
        let (mut app, _dir) = app_with_workspace();
        app.create_workspace("Action Close");
        assert!(app.workspace_sidebar.visible);

        app.handle_sidebar_action(SidebarAction::CloseWorkspace);
        assert!(!app.workspace_sidebar.visible);
    }

    #[test]
    fn test_app_handle_sidebar_action_create() {
        let (mut app, _dir) = app_with_workspace();
        app.handle_sidebar_action(SidebarAction::CreateWorkspace);
        assert!(app.workspace_sidebar.visible);
        assert_eq!(app.workspace_sidebar.workspace_name, "New Workspace");
    }

    #[test]
    fn test_app_handle_sidebar_action_rename() {
        let (mut app, _dir) = app_with_workspace();
        app.create_workspace("Old");
        let ws_id = app.workspace_sidebar.workspace_id.clone().unwrap();

        app.handle_sidebar_action(SidebarAction::RenameWorkspace(ws_id, "Renamed".to_string()));
        assert_eq!(app.workspace_sidebar.workspace_name, "Renamed");
    }

    #[test]
    fn test_app_handle_sidebar_action_delete() {
        let (mut app, _dir) = app_with_workspace();
        app.create_workspace("To Delete");
        let ws_id = app.workspace_sidebar.workspace_id.clone().unwrap();

        app.handle_sidebar_action(SidebarAction::DeleteWorkspace(ws_id));
        assert!(!app.workspace_sidebar.visible);
    }

    #[test]
    fn test_app_handle_sidebar_action_confirm_new_file() {
        let (mut app, _dir) = app_with_workspace();
        let folder = tempfile::tempdir().unwrap();
        app.create_workspace("File Action");
        app.add_folder_path_to_workspace(folder.path());

        app.handle_sidebar_action(SidebarAction::ConfirmNewFile(
            folder.path().to_path_buf(),
            "created.txt".to_string(),
        ));
        assert!(folder.path().join("created.txt").exists());
    }

    #[test]
    fn test_app_handle_sidebar_action_confirm_new_folder() {
        let (mut app, _dir) = app_with_workspace();
        let folder = tempfile::tempdir().unwrap();
        app.create_workspace("Folder Action");
        app.add_folder_path_to_workspace(folder.path());

        app.handle_sidebar_action(SidebarAction::ConfirmNewFolder(
            folder.path().to_path_buf(),
            "new_dir".to_string(),
        ));
        assert!(folder.path().join("new_dir").is_dir());
    }

    #[test]
    fn test_app_handle_sidebar_action_confirm_rename() {
        let (mut app, _dir) = app_with_workspace();
        let folder = tempfile::tempdir().unwrap();
        let file = folder.path().join("original.rs");
        std::fs::write(&file, "").unwrap();

        app.create_workspace("Rename Action");
        app.add_folder_path_to_workspace(folder.path());

        app.handle_sidebar_action(SidebarAction::ConfirmRenameEntry(
            file.clone(),
            "renamed.rs".to_string(),
        ));
        assert!(!file.exists());
        assert!(folder.path().join("renamed.rs").exists());
    }

    #[test]
    fn test_app_handle_sidebar_action_none_is_noop() {
        let (mut app, _dir) = app_with_workspace();
        app.create_workspace("Noop");
        let name_before = app.workspace_sidebar.workspace_name.clone();
        app.handle_sidebar_action(SidebarAction::None);
        assert_eq!(app.workspace_sidebar.workspace_name, name_before);
    }

    #[test]
    fn test_app_handle_sidebar_action_remove_folder() {
        let (mut app, _dir) = app_with_workspace();
        let folder = tempfile::tempdir().unwrap();
        app.create_workspace("Remove Action");
        app.add_folder_path_to_workspace(folder.path());
        assert_eq!(app.workspace_sidebar.tree.len(), 1);

        app.handle_sidebar_action(SidebarAction::RemoveFolder(folder.path().to_path_buf()));
        assert!(app.workspace_sidebar.tree.is_empty());
    }

    #[test]
    fn test_app_tick_workspace_watcher_no_watcher() {
        let (mut app, _dir) = app_with_workspace();
        // No watcher, should be a no-op
        app.tick_workspace_watcher();
        assert!(app.workspace_sidebar.tree.is_empty());
    }

    #[test]
    fn test_app_tick_workspace_watcher_no_events() {
        let (mut app, _dir) = app_with_workspace();
        let folder = tempfile::tempdir().unwrap();
        app.create_workspace("Watcher Test");
        app.add_folder_path_to_workspace(folder.path());

        // Poll immediately — no events expected
        app.tick_workspace_watcher();
        // Should not crash or change tree (beyond initial scan)
    }

    #[test]
    fn test_app_get_cached_workspace_list() {
        let (mut app, _dir) = app_with_workspace();
        app.create_workspace("WS A");
        app.create_workspace("WS B");

        let list = app.get_cached_workspace_list().clone();
        assert_eq!(list.len(), 2);
        // Should be cached now
        assert!(app.cached_workspace_list.is_some());

        // Second call returns same cached data
        let list2 = app.get_cached_workspace_list().clone();
        assert_eq!(list, list2);
    }

    #[test]
    fn test_app_invalidate_workspace_cache() {
        let (mut app, _dir) = app_with_workspace();
        app.create_workspace("Cached");
        let _ = app.get_cached_workspace_list();
        assert!(app.cached_workspace_list.is_some());

        app.invalidate_workspace_cache();
        assert!(app.cached_workspace_list.is_none());
    }

    #[test]
    fn test_app_restore_workspace_on_startup() {
        let (mut app, _dir) = app_with_workspace();
        app.create_workspace("Restore Me");
        let ws_id = app.workspace_sidebar.workspace_id.clone().unwrap();

        // Close sidebar but leave active in store
        app.workspace_sidebar.visible = false;
        app.workspace_sidebar.workspace_id = None;
        app.workspace_sidebar.workspace_name.clear();

        app.restore_workspace_on_startup();
        assert!(app.workspace_sidebar.visible);
        assert_eq!(app.workspace_sidebar.workspace_name, "Restore Me");
        assert_eq!(
            app.workspace_sidebar.workspace_id.as_deref(),
            Some(ws_id.as_str())
        );
    }

    #[test]
    fn test_app_restore_workspace_on_startup_no_active() {
        let (mut app, _dir) = app_with_workspace();
        // No active workspace set — should be a no-op
        app.restore_workspace_on_startup();
        assert!(!app.workspace_sidebar.visible);
        assert!(app.workspace_sidebar.workspace_id.is_none());
    }

    #[test]
    fn test_is_valid_simple_name_accepts_normal_names() {
        assert!(is_valid_simple_name("file.txt"));
        assert!(is_valid_simple_name("README"));
        assert!(is_valid_simple_name("my_folder"));
        assert!(is_valid_simple_name(".gitignore"));
        assert!(is_valid_simple_name("a"));
    }

    #[test]
    fn test_is_valid_simple_name_rejects_traversal() {
        assert!(!is_valid_simple_name(""));
        assert!(!is_valid_simple_name("."));
        assert!(!is_valid_simple_name(".."));
        assert!(!is_valid_simple_name("../etc/passwd"));
        assert!(!is_valid_simple_name("/etc/passwd"));
        assert!(!is_valid_simple_name("foo/bar"));
        assert!(!is_valid_simple_name("foo\\bar"));
    }

    #[test]
    fn test_is_valid_simple_name_rejects_drive_prefix() {
        assert!(!is_valid_simple_name("C:"));
        assert!(!is_valid_simple_name("C:\\Windows"));
        assert!(!is_valid_simple_name("Z:foo"));
    }

    #[test]
    fn test_is_valid_simple_name_rejects_control_chars() {
        assert!(!is_valid_simple_name("foo\0bar"));
        assert!(!is_valid_simple_name("foo\nbar"));
        assert!(!is_valid_simple_name("foo\tbar"));
        assert!(!is_valid_simple_name("foo\rbar"));
    }

    #[test]
    fn test_create_new_file_rejects_invalid_name() {
        let (mut app, _dir) = app_with_workspace();
        let folder = tempfile::tempdir().unwrap();
        app.create_workspace("Validate File");
        app.add_folder_path_to_workspace(folder.path());

        app.create_new_file_in_workspace(folder.path(), "../escape.txt");
        assert!(!folder.path().join("..").join("escape.txt").exists());
        // The original tree should be unchanged.
        let tree_len_before = app.workspace_sidebar.tree[0].entries.len();
        app.create_new_file_in_workspace(folder.path(), "/abs.txt");
        assert_eq!(app.workspace_sidebar.tree[0].entries.len(), tree_len_before);
    }

    #[test]
    fn test_create_new_folder_rejects_invalid_name() {
        let (mut app, _dir) = app_with_workspace();
        let folder = tempfile::tempdir().unwrap();
        app.create_workspace("Validate Folder");
        app.add_folder_path_to_workspace(folder.path());

        app.create_new_folder_in_workspace(folder.path(), "../escape");
        assert!(!folder.path().join("..").join("escape").exists());
    }

    #[test]
    fn test_rename_entry_rejects_invalid_name() {
        let (mut app, _dir) = app_with_workspace();
        let folder = tempfile::tempdir().unwrap();
        let file = folder.path().join("ok.txt");
        std::fs::write(&file, "x").unwrap();
        app.create_workspace("Validate Rename");
        app.add_folder_path_to_workspace(folder.path());

        app.rename_entry_in_workspace(&file, "../escape.txt");
        assert!(
            file.exists(),
            "Original file must be preserved on rejection"
        );
    }

    #[test]
    fn test_app_create_workspace_no_store() {
        let mut app = super::super::tests::test_app();
        // workspace_store is None — should return early
        app.create_workspace("No Store");
        assert!(!app.workspace_sidebar.visible);
        assert!(app.workspace_sidebar.workspace_id.is_none());
    }

    #[test]
    fn test_app_open_workspace_no_store() {
        let mut app = super::super::tests::test_app();
        app.open_workspace("some-id");
        assert!(!app.workspace_sidebar.visible);
    }

    #[test]
    fn test_app_add_folder_no_store() {
        let mut app = super::super::tests::test_app();
        let folder = tempfile::tempdir().unwrap();
        app.add_folder_path_to_workspace(folder.path());
        assert!(app.workspace_sidebar.tree.is_empty());
    }

    #[test]
    fn test_app_add_folder_no_active_workspace() {
        let (mut app, _dir) = app_with_workspace();
        // Store exists but no workspace is active
        let folder = tempfile::tempdir().unwrap();
        app.add_folder_path_to_workspace(folder.path());
        assert!(app.workspace_sidebar.tree.is_empty());
    }

    #[test]
    fn test_app_remove_folder_no_store() {
        let mut app = super::super::tests::test_app();
        app.remove_folder_from_workspace(Path::new("/nonexistent"));
        // Should not panic
    }

    #[test]
    fn test_app_rename_workspace_no_store() {
        let mut app = super::super::tests::test_app();
        app.rename_workspace("id", "name");
        // Should not panic
    }

    #[test]
    fn test_app_delete_workspace_no_store() {
        let mut app = super::super::tests::test_app();
        app.delete_workspace("id");
        // Should not panic
    }

    #[test]
    fn test_app_close_workspace_no_store() {
        let mut app = super::super::tests::test_app();
        app.close_workspace();
        // Should not panic, and store error is silently ignored
    }

    #[test]
    fn test_app_handle_sidebar_action_switch() {
        let (mut app, _dir) = app_with_workspace();
        app.create_workspace("WS One");
        app.create_workspace("WS Two");
        let entries = app
            .workspace_store
            .as_ref()
            .unwrap()
            .list_workspaces()
            .unwrap();
        let ws_one_id = entries
            .iter()
            .find(|e| e.name == "WS One")
            .unwrap()
            .id
            .clone();

        app.handle_sidebar_action(SidebarAction::SwitchWorkspace(ws_one_id));
        assert_eq!(app.workspace_sidebar.workspace_name, "WS One");
    }

    #[test]
    fn test_app_add_folder_persists_to_store() {
        let (mut app, _dir) = app_with_workspace();
        let folder = tempfile::tempdir().unwrap();
        app.create_workspace("Persist Folder");

        app.add_folder_path_to_workspace(folder.path());

        let entries = app
            .workspace_store
            .as_ref()
            .unwrap()
            .list_workspaces()
            .unwrap();
        let ws = entries.iter().find(|e| e.name == "Persist Folder").unwrap();
        assert_eq!(ws.folders.len(), 1);
        assert_eq!(ws.folders[0], folder.path().to_string_lossy().as_ref());
    }

    #[test]
    fn test_app_toggle_hidden_files() {
        let (mut app, _dir) = app_with_workspace();
        let folder = tempfile::tempdir().unwrap();
        std::fs::write(folder.path().join(".hidden"), "").unwrap();
        std::fs::write(folder.path().join("visible.txt"), "").unwrap();

        app.create_workspace("Hidden Toggle");
        app.add_folder_path_to_workspace(folder.path());

        // Default: hidden files excluded
        assert!(!app.workspace_sidebar.show_hidden);
        assert_eq!(app.workspace_sidebar.tree[0].entries.len(), 1);
        assert_eq!(app.workspace_sidebar.tree[0].entries[0].name, "visible.txt");

        // Toggle on: hidden files included
        app.toggle_hidden_files();
        assert!(app.workspace_sidebar.show_hidden);
        assert_eq!(app.workspace_sidebar.tree[0].entries.len(), 2);
        assert!(app.workspace_sidebar.tree[0]
            .entries
            .iter()
            .any(|e| e.name == ".hidden"));

        // Toggle off: hidden files excluded again
        app.toggle_hidden_files();
        assert!(!app.workspace_sidebar.show_hidden);
        assert_eq!(app.workspace_sidebar.tree[0].entries.len(), 1);
        assert_eq!(app.workspace_sidebar.tree[0].entries[0].name, "visible.txt");
    }

    #[test]
    fn test_app_handle_sidebar_action_expand_all_queues_bulk() {
        let (mut app, _dir) = app_with_workspace();
        app.create_workspace("Expand Action");
        assert!(app.workspace_sidebar.pending_bulk_collapse.is_none());
        app.handle_sidebar_action(SidebarAction::ExpandAll);
        assert_eq!(app.workspace_sidebar.pending_bulk_collapse, Some(true));
    }

    #[test]
    fn test_app_handle_sidebar_action_collapse_all_queues_bulk() {
        let (mut app, _dir) = app_with_workspace();
        app.create_workspace("Collapse Action");
        app.handle_sidebar_action(SidebarAction::CollapseAll);
        assert_eq!(app.workspace_sidebar.pending_bulk_collapse, Some(false));
    }

    #[test]
    fn test_expand_all_does_not_pre_mutate_descendant_flags() {
        // ExpandAll/CollapseAll only affect workspace roots — descendants
        // must not be touched at action-dispatch time. Cascading expansion
        // through render_directory_entry would lazy-load the entire tree
        // and freeze the UI on large workspaces.
        use crate::workspace::tree::{EntryKind, TreeEntry};

        let (mut app, _dir) = app_with_workspace();
        let folder = tempfile::tempdir().unwrap();
        std::fs::create_dir(folder.path().join("nested")).unwrap();

        app.create_workspace("Freeze Repro");
        app.add_folder_path_to_workspace(folder.path());

        // Force the nested directory entry into a collapsed state so we can
        // detect any spurious mutation of its `expanded` flag.
        let nested_idx = app.workspace_sidebar.tree[0]
            .entries
            .iter()
            .position(|e| e.kind == EntryKind::Directory && e.name == "nested")
            .expect("nested directory should be in the scanned tree");
        app.workspace_sidebar.tree[0].entries[nested_idx].expanded = false;

        // Sanity-check that descendant flag is false before the action.
        assert!(!app.workspace_sidebar.tree[0].entries[nested_idx].expanded);

        app.handle_sidebar_action(SidebarAction::ExpandAll);

        // Action only queues the bulk flag; descendant flag must remain
        // untouched until render_tree consumes it on the root only.
        assert_eq!(app.workspace_sidebar.pending_bulk_collapse, Some(true));
        let nested: &TreeEntry = &app.workspace_sidebar.tree[0].entries[nested_idx];
        assert!(
            !nested.expanded,
            "ExpandAll must not pre-mutate descendant directory flags"
        );
    }

    #[test]
    fn test_app_handle_sidebar_action_toggle_hidden() {
        let (mut app, _dir) = app_with_workspace();
        let folder = tempfile::tempdir().unwrap();
        std::fs::write(folder.path().join(".dotfile"), "").unwrap();
        std::fs::write(folder.path().join("normal.txt"), "").unwrap();

        app.create_workspace("Toggle Action");
        app.add_folder_path_to_workspace(folder.path());

        assert!(!app.workspace_sidebar.show_hidden);
        app.handle_sidebar_action(SidebarAction::ToggleHiddenFiles);
        assert!(app.workspace_sidebar.show_hidden);
        assert_eq!(app.workspace_sidebar.tree[0].entries.len(), 2);
    }
}
