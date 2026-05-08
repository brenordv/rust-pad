/// Workspace sidebar UI rendering.
use std::path::{Path, PathBuf};

use eframe::egui;

use super::tree::{EntryKind, FolderRoot, TreeEntry};
use super::watcher::WorkspaceWatcher;
use crate::app::workspace_ops::generate_unique_name;

/// Minimum sidebar width in pixels.
const MIN_WIDTH: f32 = 150.0;
/// Maximum sidebar width in pixels.
const MAX_WIDTH: f32 = 500.0;
/// Default sidebar width in pixels.
const DEFAULT_WIDTH: f32 = 250.0;
/// Space reserved for toolbar buttons (close, add folder) in the header row.
const HEADER_TOOLBAR_RESERVED: f32 = 80.0;

/// Actions the sidebar can request from the main application.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SidebarAction {
    /// Open a file in the editor.
    OpenFile(PathBuf),
    /// Delete a file (send to trash).
    DeleteFile(PathBuf),
    /// Trigger the "Add Folder" dialog.
    AddFolder,
    /// Remove a folder from the workspace (not from disk).
    RemoveFolder(PathBuf),
    /// Switch to a different workspace by ID.
    SwitchWorkspace(String),
    /// Close the active workspace.
    CloseWorkspace,
    /// Create a new workspace.
    CreateWorkspace,
    /// Rename a workspace (id, new_name).
    RenameWorkspace(String, String),
    /// Delete a workspace by ID.
    DeleteWorkspace(String),
    /// Confirm creation of a new file (parent_dir, file_name).
    ConfirmNewFile(PathBuf, String),
    /// Confirm creation of a new folder (parent_dir, folder_name).
    ConfirmNewFolder(PathBuf, String),
    /// Confirm rename of a file or folder (original_path, new_name).
    ConfirmRenameEntry(PathBuf, String),
    /// No action.
    None,
}

/// State for inline creation of a new file or folder.
#[derive(Debug, Clone)]
pub(crate) struct NewEntryState {
    /// Directory where the new entry will be created.
    pub parent: PathBuf,
    /// Current name in the text field.
    pub name: String,
    /// True if creating a directory, false for a file.
    pub is_dir: bool,
}

/// State for inline rename of a file or folder.
#[derive(Debug, Clone)]
pub(crate) struct RenameEntryState {
    /// Original full path of the entry being renamed.
    pub original_path: PathBuf,
    /// Current name in the text field.
    pub name: String,
    /// True if this is a directory.
    pub is_dir: bool,
}

/// State for the workspace sidebar panel.
#[derive(Debug)]
pub struct WorkspaceSidebar {
    /// Whether the sidebar is visible.
    pub visible: bool,
    /// Current sidebar width.
    pub width: f32,
    /// Tree of folder roots and their entries.
    pub tree: Vec<FolderRoot>,
    /// Filesystem watcher (created when a workspace is opened).
    pub watcher: Option<WorkspaceWatcher>,
    /// Name of the active workspace (for display in the header).
    pub workspace_name: String,
    /// ID of the active workspace.
    pub workspace_id: Option<String>,
    /// Available workspaces for the context menu (id, name).
    /// Populated by `App` before each render pass.
    pub(crate) available_workspaces: Vec<(String, String)>,
    /// Inline rename state: Some(current_text) when renaming the workspace.
    pub(crate) rename_buffer: Option<String>,
    /// Set to true on the frame where Enter confirms any inline edit. Cleared next frame.
    /// This prevents the Enter key from propagating to the editor.
    pub(crate) rename_just_confirmed: bool,
    /// Inline state for creating a new file or folder.
    pub(crate) new_entry: Option<NewEntryState>,
    /// Inline state for renaming a file or folder.
    pub(crate) rename_entry: Option<RenameEntryState>,
}

impl Default for WorkspaceSidebar {
    fn default() -> Self {
        Self::new()
    }
}

impl WorkspaceSidebar {
    /// Creates a new sidebar in hidden state.
    pub fn new() -> Self {
        Self {
            visible: false,
            width: DEFAULT_WIDTH,
            tree: Vec::new(),
            watcher: None,
            workspace_name: String::new(),
            workspace_id: None,
            available_workspaces: Vec::new(),
            rename_buffer: None,
            rename_just_confirmed: false,
            new_entry: None,
            rename_entry: None,
        }
    }

    /// Sets the sidebar width, clamping to valid bounds.
    pub fn set_width(&mut self, width: f32) {
        self.width = width.clamp(MIN_WIDTH, MAX_WIDTH);
    }

    /// Returns the clamped width.
    pub fn width(&self) -> f32 {
        self.width.clamp(MIN_WIDTH, MAX_WIDTH)
    }

    /// Returns true if the sidebar should be rendered.
    pub fn is_visible(&self) -> bool {
        self.visible
    }

    /// Renders the sidebar content and returns any action to execute.
    pub fn show(&mut self, ui: &mut egui::Ui) -> SidebarAction {
        // Clear the Enter-suppression flag from the previous frame.
        self.rename_just_confirmed = false;

        let mut action = SidebarAction::None;

        // Header: workspace name + toolbar
        self.render_header(ui, &mut action);

        ui.separator();

        // Tree view (scrollable)
        egui::ScrollArea::both()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                if self.tree.is_empty() {
                    ui.vertical_centered(|ui| {
                        ui.add_space(20.0);
                        ui.label("No folders in workspace.");
                        ui.add_space(8.0);
                        if ui.button("Add Folder...").clicked() {
                            action = SidebarAction::AddFolder;
                        }
                    });
                } else {
                    self.render_tree(ui, &mut action);
                }
            });

        action
    }

    /// Renders the sidebar header with workspace name and toolbar buttons.
    fn render_header(&mut self, ui: &mut egui::Ui, action: &mut SidebarAction) {
        ui.horizontal(|ui| {
            if self.workspace_name.is_empty() {
                ui.strong("Workspace");
            } else if self.rename_buffer.is_some() {
                self.render_workspace_rename_field(ui, action);
            } else {
                self.render_workspace_name_with_menu(ui, action);
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .small_button("\u{2715}")
                    .on_hover_text("Close workspace")
                    .clicked()
                {
                    *action = SidebarAction::CloseWorkspace;
                }
                if ui.small_button("+").on_hover_text("Add folder").clicked() {
                    *action = SidebarAction::AddFolder;
                }
            });
        });
    }

    /// Renders the inline text field for renaming the workspace.
    fn render_workspace_rename_field(&mut self, ui: &mut egui::Ui, action: &mut SidebarAction) {
        let buf = self.rename_buffer.as_mut().unwrap();
        let available = ui.available_width() - HEADER_TOOLBAR_RESERVED;
        let desired = available.max(80.0);
        let response = ui.add(egui::TextEdit::singleline(buf).desired_width(desired));
        ui.add_space(8.0);
        if !response.has_focus() && !response.lost_focus() {
            response.request_focus();
        }
        if response.lost_focus() {
            if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                if let Some(id) = &self.workspace_id {
                    let new_name = buf.clone();
                    *action = SidebarAction::RenameWorkspace(id.clone(), new_name.clone());
                    self.workspace_name.clone_from(&new_name);
                }
                self.rename_just_confirmed = true;
            }
            self.rename_buffer = None;
        }
        if response.ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.rename_buffer = None;
        }
    }

    /// Renders the workspace name label with context menu for workspace operations.
    fn render_workspace_name_with_menu(&mut self, ui: &mut egui::Ui, action: &mut SidebarAction) {
        let name_response = ui
            .strong(&self.workspace_name)
            .on_hover_text("Double-click to rename");
        if name_response.double_clicked() {
            self.rename_buffer = Some(self.workspace_name.clone());
        }
        name_response.context_menu(|ui| {
            if ui.button("New Workspace...").clicked() {
                *action = SidebarAction::CreateWorkspace;
                ui.close();
            }
            if !self.available_workspaces.is_empty() {
                ui.menu_button("Open Workspace", |ui| {
                    let active_id = self.workspace_id.as_deref().unwrap_or("");
                    for (ws_id, ws_name) in &self.available_workspaces {
                        let is_active = ws_id == active_id;
                        let label = if is_active {
                            format!("\u{2713} {ws_name}")
                        } else {
                            ws_name.clone()
                        };
                        if ui.button(&label).clicked() {
                            if !is_active {
                                *action = SidebarAction::SwitchWorkspace(ws_id.clone());
                            }
                            ui.close();
                        }
                    }
                });
            }
            ui.separator();
            if ui.button("Close Workspace").clicked() {
                *action = SidebarAction::CloseWorkspace;
                ui.close();
            }
        });
    }

    /// Renders the folder tree with collapsible roots.
    fn render_tree(&mut self, ui: &mut egui::Ui, action: &mut SidebarAction) {
        let mut context_action = SidebarAction::None;
        let mut new_entry_request: Option<NewEntryState> = None;
        let tree_len = self.tree.len();

        for root_idx in 0..tree_len {
            let root_path = self.tree[root_idx].path.clone();
            let root_name = root_path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| root_path.to_string_lossy().into_owned());

            let folder_exists = root_path.is_dir();

            let id = ui.make_persistent_id(format!("root_{root_idx}"));
            let expanded = self.tree[root_idx].expanded;

            // Force-open the root if the inline new entry targets it
            let should_force_open = self
                .new_entry
                .as_ref()
                .is_some_and(|ne| ne.parent == root_path);

            let mut cs = egui::collapsing_header::CollapsingState::load_with_default_open(
                ui.ctx(),
                id,
                expanded,
            );
            if should_force_open && !cs.is_open() {
                cs.set_open(true);
            }

            let (_toggle, header_inner, _body) = cs
                .show_header(ui, |ui| {
                    let response = if folder_exists {
                        ui.add(
                            egui::Label::new(egui::RichText::new(&root_name).strong())
                                .sense(egui::Sense::click()),
                        )
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                    } else {
                        ui.weak(format!("\u{26A0} {root_name} (unavailable)"))
                    };
                    response.context_menu(|ui| {
                        if folder_exists {
                            show_new_entry_menu(ui, &root_path, &mut new_entry_request);
                            ui.separator();
                        }
                        if ui.button("Remove from Workspace").clicked() {
                            context_action = SidebarAction::RemoveFolder(root_path.clone());
                            ui.close();
                        }
                    });
                    response
                })
                .body(|ui| {
                    if folder_exists {
                        render_entry_list(
                            ui,
                            &root_path,
                            &mut self.tree[root_idx].entries,
                            action,
                            &mut self.new_entry,
                            &mut self.rename_entry,
                            &mut self.rename_just_confirmed,
                        );
                    } else {
                        ui.weak("Folder not found or inaccessible");
                    }
                });

            // Double-click on the header label toggles expand/collapse
            if header_inner.inner.double_clicked() {
                if let Some(mut state) =
                    egui::collapsing_header::CollapsingState::load(ui.ctx(), id)
                {
                    let new_open = !state.is_open();
                    state.set_open(new_open);
                    state.store(ui.ctx());
                    self.tree[root_idx].expanded = new_open;
                }
            }

            // Persist expanded state from egui's CollapsingState
            self.tree[root_idx].expanded =
                egui::collapsing_header::CollapsingState::load(ui.ctx(), id)
                    .map_or(expanded, |s| s.is_open());
        }

        if context_action != SidebarAction::None {
            *action = context_action;
        }
        if let Some(req) = new_entry_request {
            self.new_entry = Some(req);
            self.rename_entry = None;
        }
    }
}

/// Renders the inline rename text field for a tree entry.
/// Returns `true` when the rename interaction is complete.
fn render_inline_rename(
    ui: &mut egui::Ui,
    state: &mut RenameEntryState,
    action: &mut SidebarAction,
    rename_just_confirmed: &mut bool,
) -> bool {
    let original_name = state
        .original_path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let icon = if state.is_dir {
        "\u{1F4C1}"
    } else {
        file_icon(&original_name)
    };
    ui.horizontal(|ui| {
        ui.label(icon);
        let resp =
            ui.add(egui::TextEdit::singleline(&mut state.name).desired_width(ui.available_width()));
        if !resp.has_focus() && !resp.lost_focus() {
            resp.request_focus();
        }
        if resp.lost_focus() {
            let name = state.name.trim().to_string();
            if ui.input(|i| i.key_pressed(egui::Key::Enter))
                && !name.is_empty()
                && name != original_name
            {
                *action = SidebarAction::ConfirmRenameEntry(state.original_path.clone(), name);
                *rename_just_confirmed = true;
            }
            return true;
        }
        if resp.ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            return true;
        }
        false
    })
    .inner
}

/// Renders a directory tree entry with collapsing header, context menu, and lazy-loaded children.
#[allow(clippy::too_many_arguments)]
fn render_directory_entry(
    ui: &mut egui::Ui,
    entry: &mut TreeEntry,
    action: &mut SidebarAction,
    new_entry: &mut Option<NewEntryState>,
    rename_entry: &mut Option<RenameEntryState>,
    rename_just_confirmed: &mut bool,
    new_entry_request: &mut Option<NewEntryState>,
    rename_request: &mut Option<RenameEntryState>,
) {
    let name = entry.name.clone();
    let path = entry.path.clone();
    let expanded = entry.expanded;
    let id = ui.make_persistent_id(("entry", &path));

    let should_force_open = new_entry.as_ref().is_some_and(|ne| ne.parent == path);

    let mut cs =
        egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, expanded);
    if should_force_open && !cs.is_open() {
        cs.set_open(true);
    }

    let (_toggle, header_inner, _body) = cs
        .show_header(ui, |ui| {
            let response = ui
                .add(egui::Label::new(format!("\u{1F4C1} {name}")).sense(egui::Sense::click()))
                .on_hover_cursor(egui::CursorIcon::PointingHand);
            response.context_menu(|ui| {
                show_new_entry_menu(ui, &path, new_entry_request);
                ui.separator();
                if ui.button("Rename").clicked() {
                    *rename_request = Some(RenameEntryState {
                        original_path: path.clone(),
                        name: name.clone(),
                        is_dir: true,
                    });
                    ui.close();
                }
                if ui.button("Delete").clicked() {
                    *action = SidebarAction::DeleteFile(path.clone());
                    ui.close();
                }
            });
            response
        })
        .body(|ui| {
            // Lazy-load children on first expand. This blocks the UI
            // thread for one frame while scanning, but the result is
            // cached in `entry.children` so subsequent frames are free.
            if entry.children.is_empty() {
                let dir_path = entry.path.clone();
                if let Ok(children) = super::scanner::scan_directory(&dir_path) {
                    entry.children = children;
                }
            }
            render_entry_list(
                ui,
                &path,
                &mut entry.children,
                action,
                new_entry,
                rename_entry,
                rename_just_confirmed,
            );
        });

    if header_inner.inner.double_clicked() {
        if let Some(mut state) = egui::collapsing_header::CollapsingState::load(ui.ctx(), id) {
            let new_open = !state.is_open();
            state.set_open(new_open);
            state.store(ui.ctx());
            entry.expanded = new_open;
        }
    }

    entry.expanded = egui::collapsing_header::CollapsingState::load(ui.ctx(), id)
        .map_or(expanded, |s| s.is_open());
}

/// Renders a file tree entry with context menu for open, rename, and delete.
fn render_file_entry(
    ui: &mut egui::Ui,
    entry: &TreeEntry,
    action: &mut SidebarAction,
    rename_request: &mut Option<RenameEntryState>,
) {
    let icon = file_icon(&entry.name);
    let response = ui.selectable_label(false, format!("{icon} {}", entry.name));

    if response.double_clicked() {
        *action = SidebarAction::OpenFile(entry.path.clone());
    }
    response.context_menu(|ui| {
        if ui.button("Open").clicked() {
            *action = SidebarAction::OpenFile(entry.path.clone());
            ui.close();
        }
        if ui.button("Rename").clicked() {
            *rename_request = Some(RenameEntryState {
                original_path: entry.path.clone(),
                name: entry.name.clone(),
                is_dir: false,
            });
            ui.close();
        }
        if ui.button("Delete").clicked() {
            *action = SidebarAction::DeleteFile(entry.path.clone());
            ui.close();
        }
    });
}

/// Renders the inline text field for creating a new file or folder.
/// Returns `true` when the interaction is complete.
fn render_inline_new_entry_field(
    ui: &mut egui::Ui,
    state: &mut NewEntryState,
    action: &mut SidebarAction,
    rename_just_confirmed: &mut bool,
) -> bool {
    let icon = if state.is_dir {
        "\u{1F4C1}"
    } else {
        "\u{1F4C4}"
    };
    ui.horizontal(|ui| {
        ui.label(icon);
        let resp =
            ui.add(egui::TextEdit::singleline(&mut state.name).desired_width(ui.available_width()));
        if !resp.has_focus() && !resp.lost_focus() {
            resp.request_focus();
        }
        if resp.lost_focus() {
            let name = state.name.trim().to_string();
            if ui.input(|i| i.key_pressed(egui::Key::Enter)) && !name.is_empty() {
                if state.is_dir {
                    *action = SidebarAction::ConfirmNewFolder(state.parent.clone(), name);
                } else {
                    *action = SidebarAction::ConfirmNewFile(state.parent.clone(), name);
                }
                *rename_just_confirmed = true;
            }
            return true;
        }
        if resp.ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            return true;
        }
        false
    })
    .inner
}

/// Renders a slice of tree entries recursively, with lazy-loading of children.
///
/// Works at any nesting depth — directories lazy-load their children on first
/// expand and cache the result in `TreeEntry.children`.
fn render_entry_list(
    ui: &mut egui::Ui,
    parent_path: &Path,
    entries: &mut [TreeEntry],
    action: &mut SidebarAction,
    new_entry: &mut Option<NewEntryState>,
    rename_entry: &mut Option<RenameEntryState>,
    rename_just_confirmed: &mut bool,
) {
    let mut new_entry_request: Option<NewEntryState> = None;
    let mut rename_request: Option<RenameEntryState> = None;
    let mut clear_rename = false;

    for entry in entries.iter_mut() {
        let is_renaming = rename_entry
            .as_ref()
            .is_some_and(|r| r.original_path == entry.path);

        if is_renaming {
            if let Some(ref mut state) = rename_entry {
                if render_inline_rename(ui, state, action, rename_just_confirmed) {
                    clear_rename = true;
                }
            }
            continue;
        }

        match entry.kind {
            EntryKind::Directory => {
                render_directory_entry(
                    ui,
                    entry,
                    action,
                    new_entry,
                    rename_entry,
                    rename_just_confirmed,
                    &mut new_entry_request,
                    &mut rename_request,
                );
            }
            EntryKind::File => {
                render_file_entry(ui, entry, action, &mut rename_request);
            }
        }
    }

    // Inline new entry text field (at the end of the list)
    let mut clear_new = false;
    if let Some(ref mut state) = new_entry {
        if state.parent.as_path() == parent_path
            && render_inline_new_entry_field(ui, state, action, rename_just_confirmed)
        {
            clear_new = true;
        }
    }

    // Apply deferred state changes
    if let Some(req) = new_entry_request {
        *new_entry = Some(req);
        *rename_entry = None;
    }
    if let Some(req) = rename_request {
        *rename_entry = Some(req);
        *new_entry = None;
    }
    if clear_rename {
        *rename_entry = None;
    }
    if clear_new {
        *new_entry = None;
    }
}

/// Renders "New File..." and "New Folder..." context menu items.
///
/// Used in both root folder and subdirectory context menus to avoid duplication.
fn show_new_entry_menu(
    ui: &mut egui::Ui,
    parent: &Path,
    new_entry_request: &mut Option<NewEntryState>,
) {
    if ui.button("New File...").clicked() {
        let name = generate_unique_name(parent, "new_file.txt", false);
        *new_entry_request = Some(NewEntryState {
            parent: parent.to_path_buf(),
            name,
            is_dir: false,
        });
        ui.close();
    }
    if ui.button("New Folder...").clicked() {
        let name = generate_unique_name(parent, "new_folder", true);
        *new_entry_request = Some(NewEntryState {
            parent: parent.to_path_buf(),
            name,
            is_dir: true,
        });
        ui.close();
    }
}

/// Returns a simple text icon based on file extension.
fn file_icon(name: &str) -> &'static str {
    let ext = name.rsplit('.').next().unwrap_or("");
    match ext.to_lowercase().as_str() {
        "rs" => "\u{1F9E0}",
        "toml" | "yaml" | "yml" | "json" | "xml" => "\u{2699}",
        "md" | "txt" | "log" => "\u{1F4DD}",
        "png" | "jpg" | "jpeg" | "gif" | "svg" | "ico" => "\u{1F5BC}",
        "lock" => "\u{1F512}",
        _ => "\u{1F4C4}",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sidebar_default_state() {
        let sidebar = WorkspaceSidebar::new();
        assert!(!sidebar.visible);
        assert_eq!(sidebar.width, DEFAULT_WIDTH);
        assert!(sidebar.tree.is_empty());
        assert!(sidebar.watcher.is_none());
        assert!(sidebar.workspace_id.is_none());
        assert!(sidebar.rename_buffer.is_none());
        assert!(!sidebar.rename_just_confirmed);
        assert!(sidebar.new_entry.is_none());
        assert!(sidebar.rename_entry.is_none());
    }

    #[test]
    fn test_sidebar_width_clamping() {
        let mut sidebar = WorkspaceSidebar::new();

        sidebar.set_width(100.0); // Below min
        assert_eq!(sidebar.width(), MIN_WIDTH);

        sidebar.set_width(600.0); // Above max
        assert_eq!(sidebar.width(), MAX_WIDTH);

        sidebar.set_width(300.0); // Within range
        assert_eq!(sidebar.width(), 300.0);
    }

    #[test]
    fn test_sidebar_action_variants_distinct() {
        let actions = vec![
            SidebarAction::OpenFile(PathBuf::from("/a")),
            SidebarAction::DeleteFile(PathBuf::from("/b")),
            SidebarAction::AddFolder,
            SidebarAction::RemoveFolder(PathBuf::from("/c")),
            SidebarAction::SwitchWorkspace("ws-1".to_string()),
            SidebarAction::CloseWorkspace,
            SidebarAction::CreateWorkspace,
            SidebarAction::RenameWorkspace("ws-1".to_string(), "New".to_string()),
            SidebarAction::DeleteWorkspace("ws-1".to_string()),
            SidebarAction::ConfirmNewFile(PathBuf::from("/d"), "file.txt".to_string()),
            SidebarAction::ConfirmNewFolder(PathBuf::from("/e"), "folder".to_string()),
            SidebarAction::ConfirmRenameEntry(PathBuf::from("/f"), "new_name".to_string()),
            SidebarAction::None,
        ];

        // All variants are distinct
        for (i, a) in actions.iter().enumerate() {
            for (j, b) in actions.iter().enumerate() {
                if i != j {
                    assert_ne!(a, b);
                }
            }
        }
    }

    #[test]
    fn test_rename_just_confirmed_suppresses_editor_input() {
        let mut sidebar = WorkspaceSidebar::new();
        sidebar.workspace_id = Some("ws-1".to_string());
        sidebar.workspace_name = "Old Name".to_string();

        // Simulate entering rename mode
        sidebar.rename_buffer = Some("Old Name".to_string());
        assert!(sidebar.rename_buffer.is_some());
        assert!(!sidebar.rename_just_confirmed);

        // Simulate Enter confirmation: buffer cleared, flag set
        sidebar.rename_buffer = None;
        sidebar.rename_just_confirmed = true;

        // Even though rename_buffer is None, the flag signals suppression
        assert!(sidebar.rename_buffer.is_none());
        assert!(sidebar.rename_just_confirmed);

        // Next frame: flag is reset
        sidebar.rename_just_confirmed = false;
        assert!(!sidebar.rename_just_confirmed);
    }

    #[test]
    fn test_rename_escape_does_not_set_confirmed_flag() {
        let mut sidebar = WorkspaceSidebar::new();
        sidebar.workspace_id = Some("ws-1".to_string());
        sidebar.workspace_name = "My Workspace".to_string();

        // Enter rename mode
        sidebar.rename_buffer = Some("My Workspace".to_string());

        // Simulate Escape: buffer cleared, flag NOT set
        sidebar.rename_buffer = None;
        // Escape path does not set rename_just_confirmed
        assert!(!sidebar.rename_just_confirmed);
    }

    #[test]
    fn test_file_icon_known_extensions() {
        assert_eq!(file_icon("main.rs"), "\u{1F9E0}");
        assert_eq!(file_icon("Cargo.toml"), "\u{2699}");
        assert_eq!(file_icon("README.md"), "\u{1F4DD}");
        assert_eq!(file_icon("logo.png"), "\u{1F5BC}");
        assert_eq!(file_icon("Cargo.lock"), "\u{1F512}");
        assert_eq!(file_icon("unknown.xyz"), "\u{1F4C4}");
    }

    #[test]
    fn test_new_entry_state_creation() {
        let state = NewEntryState {
            parent: PathBuf::from("/project/src"),
            name: "new_file.txt".to_string(),
            is_dir: false,
        };
        assert_eq!(state.parent, PathBuf::from("/project/src"));
        assert_eq!(state.name, "new_file.txt");
        assert!(!state.is_dir);
    }

    #[test]
    fn test_rename_entry_state_creation() {
        let state = RenameEntryState {
            original_path: PathBuf::from("/project/src/old.rs"),
            name: "old.rs".to_string(),
            is_dir: false,
        };
        assert_eq!(state.original_path, PathBuf::from("/project/src/old.rs"));
        assert_eq!(state.name, "old.rs");
        assert!(!state.is_dir);
    }

    #[test]
    fn test_sidebar_is_visible_false_by_default() {
        let sidebar = WorkspaceSidebar::new();
        assert!(!sidebar.is_visible());
    }

    #[test]
    fn test_sidebar_default_equals_new() {
        let from_new = WorkspaceSidebar::new();
        let from_default = WorkspaceSidebar::default();
        assert_eq!(from_new.visible, from_default.visible);
        assert_eq!(from_new.width, from_default.width);
        assert!(from_default.tree.is_empty());
        assert!(from_default.workspace_id.is_none());
    }

    #[test]
    fn test_sidebar_width_exact_boundaries() {
        let mut sidebar = WorkspaceSidebar::new();

        sidebar.set_width(MIN_WIDTH);
        assert_eq!(sidebar.width(), MIN_WIDTH);

        sidebar.set_width(MAX_WIDTH);
        assert_eq!(sidebar.width(), MAX_WIDTH);

        sidebar.set_width(MIN_WIDTH - 0.01);
        assert_eq!(sidebar.width(), MIN_WIDTH);

        sidebar.set_width(MAX_WIDTH + 0.01);
        assert_eq!(sidebar.width(), MAX_WIDTH);
    }

    #[test]
    fn test_file_icon_case_insensitive() {
        assert_eq!(file_icon("main.RS"), "\u{1F9E0}");
        assert_eq!(file_icon("config.TOML"), "\u{2699}");
        assert_eq!(file_icon("image.PNG"), "\u{1F5BC}");
    }

    #[test]
    fn test_file_icon_no_extension() {
        // File with no extension should return default icon
        assert_eq!(file_icon("Makefile"), "\u{1F4C4}");
    }

    #[test]
    fn test_sidebar_visibility_toggle() {
        let mut sidebar = WorkspaceSidebar::new();
        assert!(!sidebar.is_visible());

        sidebar.visible = true;
        assert!(sidebar.is_visible());

        sidebar.visible = false;
        assert!(!sidebar.is_visible());
    }

    #[test]
    fn test_sidebar_tree_management() {
        use crate::workspace::tree::FolderRoot;

        let mut sidebar = WorkspaceSidebar::new();
        assert!(sidebar.tree.is_empty());

        sidebar.tree.push(FolderRoot {
            path: PathBuf::from("/project"),
            entries: Vec::new(),
            expanded: true,
        });
        assert_eq!(sidebar.tree.len(), 1);

        sidebar.tree.clear();
        assert!(sidebar.tree.is_empty());
    }

    #[test]
    fn test_sidebar_workspace_state() {
        let mut sidebar = WorkspaceSidebar::new();
        assert!(sidebar.workspace_id.is_none());
        assert!(sidebar.workspace_name.is_empty());

        sidebar.workspace_id = Some("ws-123".to_string());
        sidebar.workspace_name = "My Project".to_string();
        assert_eq!(sidebar.workspace_id.as_deref(), Some("ws-123"));
        assert_eq!(sidebar.workspace_name, "My Project");
    }

    #[test]
    fn test_sidebar_available_workspaces() {
        let mut sidebar = WorkspaceSidebar::new();
        assert!(sidebar.available_workspaces.is_empty());

        sidebar.available_workspaces = vec![
            ("id1".to_string(), "Workspace 1".to_string()),
            ("id2".to_string(), "Workspace 2".to_string()),
        ];
        assert_eq!(sidebar.available_workspaces.len(), 2);
    }

    #[test]
    fn test_new_entry_state_dir() {
        let state = NewEntryState {
            parent: PathBuf::from("/project/src"),
            name: "new_folder".to_string(),
            is_dir: true,
        };
        assert!(state.is_dir);
        assert_eq!(state.name, "new_folder");
    }

    #[test]
    fn test_rename_entry_state_dir() {
        let state = RenameEntryState {
            original_path: PathBuf::from("/project/src"),
            name: "src".to_string(),
            is_dir: true,
        };
        assert!(state.is_dir);
        assert_eq!(state.name, "src");
    }

    #[test]
    fn test_sidebar_rename_buffer_flow() {
        let mut sidebar = WorkspaceSidebar::new();
        sidebar.workspace_name = "Original".to_string();

        // Enter rename mode
        sidebar.rename_buffer = Some(sidebar.workspace_name.clone());
        assert_eq!(sidebar.rename_buffer.as_deref(), Some("Original"));

        // Simulate typing a new name
        if let Some(ref mut buf) = sidebar.rename_buffer {
            buf.clear();
            buf.push_str("New Name");
        }
        assert_eq!(sidebar.rename_buffer.as_deref(), Some("New Name"));

        // Cancel rename (Escape)
        sidebar.rename_buffer = None;
        assert!(sidebar.rename_buffer.is_none());
        // Original name preserved
        assert_eq!(sidebar.workspace_name, "Original");
    }

    #[test]
    fn test_sidebar_new_and_rename_entry_mutual_exclusion() {
        let mut sidebar = WorkspaceSidebar::new();

        // Set new entry state
        sidebar.new_entry = Some(NewEntryState {
            parent: PathBuf::from("/project"),
            name: "file.txt".to_string(),
            is_dir: false,
        });
        assert!(sidebar.new_entry.is_some());
        assert!(sidebar.rename_entry.is_none());

        // Switch to rename state (should conceptually clear new_entry)
        sidebar.rename_entry = Some(RenameEntryState {
            original_path: PathBuf::from("/project/old.rs"),
            name: "old.rs".to_string(),
            is_dir: false,
        });
        sidebar.new_entry = None;
        assert!(sidebar.new_entry.is_none());
        assert!(sidebar.rename_entry.is_some());
    }

    #[test]
    fn test_file_icon_all_image_types() {
        assert_eq!(file_icon("photo.jpg"), "\u{1F5BC}");
        assert_eq!(file_icon("photo.jpeg"), "\u{1F5BC}");
        assert_eq!(file_icon("animation.gif"), "\u{1F5BC}");
        assert_eq!(file_icon("vector.svg"), "\u{1F5BC}");
        assert_eq!(file_icon("favicon.ico"), "\u{1F5BC}");
    }

    #[test]
    fn test_file_icon_config_types() {
        assert_eq!(file_icon("config.yaml"), "\u{2699}");
        assert_eq!(file_icon("config.yml"), "\u{2699}");
        assert_eq!(file_icon("data.json"), "\u{2699}");
        assert_eq!(file_icon("pom.xml"), "\u{2699}");
    }

    #[test]
    fn test_file_icon_text_types() {
        assert_eq!(file_icon("notes.txt"), "\u{1F4DD}");
        assert_eq!(file_icon("app.log"), "\u{1F4DD}");
    }

    #[test]
    fn test_sidebar_action_debug() {
        let action = SidebarAction::OpenFile(PathBuf::from("/test"));
        let debug = format!("{action:?}");
        assert!(debug.contains("OpenFile"));
    }

    #[test]
    fn test_sidebar_action_clone() {
        let action = SidebarAction::RenameWorkspace("id".to_string(), "name".to_string());
        let cloned = action.clone();
        assert_eq!(action, cloned);
    }

    #[test]
    fn test_new_entry_state_clone() {
        let state = NewEntryState {
            parent: PathBuf::from("/a"),
            name: "b".to_string(),
            is_dir: false,
        };
        let cloned = state.clone();
        assert_eq!(state.parent, cloned.parent);
        assert_eq!(state.name, cloned.name);
        assert_eq!(state.is_dir, cloned.is_dir);
    }

    #[test]
    fn test_rename_entry_state_clone() {
        let state = RenameEntryState {
            original_path: PathBuf::from("/a/b"),
            name: "b".to_string(),
            is_dir: true,
        };
        let cloned = state.clone();
        assert_eq!(state.original_path, cloned.original_path);
        assert_eq!(state.name, cloned.name);
        assert_eq!(state.is_dir, cloned.is_dir);
    }
}
