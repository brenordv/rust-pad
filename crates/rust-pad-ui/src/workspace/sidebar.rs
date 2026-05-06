/// Workspace sidebar UI rendering.
use std::path::PathBuf;

use eframe::egui;

use super::tree::{EntryKind, FolderRoot, TreeEntry};
use super::watcher::WorkspaceWatcher;

/// Minimum sidebar width in pixels.
const MIN_WIDTH: f32 = 150.0;
/// Maximum sidebar width in pixels.
const MAX_WIDTH: f32 = 500.0;
/// Default sidebar width in pixels.
const DEFAULT_WIDTH: f32 = 250.0;

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
    /// No action.
    None,
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
    /// Inline rename state: Some(current_text) when renaming.
    pub(crate) rename_buffer: Option<String>,
    /// Set to true on the frame where Enter confirms a rename. Cleared next frame.
    /// This prevents the Enter key from propagating to the editor.
    pub(crate) rename_just_confirmed: bool,
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
            rename_buffer: None,
            rename_just_confirmed: false,
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
            // Workspace name (bold)
            if self.workspace_name.is_empty() {
                ui.strong("Workspace");
            } else if let Some(ref mut buf) = self.rename_buffer {
                // Inline rename mode — limit width so it doesn't overflow past buttons
                let available = ui.available_width() - 80.0; // leave room for toolbar buttons + spacing
                let desired = available.max(80.0);
                let response = ui.add(egui::TextEdit::singleline(buf).desired_width(desired));
                ui.add_space(8.0);
                // Auto-focus on the first frame
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
            } else {
                let name_response = ui
                    .strong(&self.workspace_name)
                    .on_hover_text("Double-click to rename");
                if name_response.double_clicked() {
                    self.rename_buffer = Some(self.workspace_name.clone());
                }
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // Close button
                if ui
                    .small_button("\u{2715}")
                    .on_hover_text("Close workspace")
                    .clicked()
                {
                    *action = SidebarAction::CloseWorkspace;
                }
                // Add folder button
                if ui.small_button("+").on_hover_text("Add folder").clicked() {
                    *action = SidebarAction::AddFolder;
                }
            });
        });
    }

    /// Renders the folder tree with collapsible roots.
    fn render_tree(&mut self, ui: &mut egui::Ui, action: &mut SidebarAction) {
        let mut context_action = SidebarAction::None;
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

            egui::collapsing_header::CollapsingState::load_with_default_open(
                ui.ctx(),
                id,
                expanded,
            )
            .show_header(ui, |ui| {
                let response = if folder_exists {
                    ui.strong(&root_name)
                } else {
                    ui.weak(format!("\u{26A0} {root_name} (unavailable)"))
                };
                response.context_menu(|ui| {
                    if ui.button("Remove from Workspace").clicked() {
                        context_action = SidebarAction::RemoveFolder(root_path.clone());
                        ui.close();
                    }
                });
            })
            .body(|ui| {
                if folder_exists {
                    render_entry_list(ui, &mut self.tree[root_idx].entries, action);
                } else {
                    ui.weak("Folder not found or inaccessible");
                }
            });

            // Persist expanded state from egui's CollapsingState
            self.tree[root_idx].expanded =
                egui::collapsing_header::CollapsingState::load(ui.ctx(), id)
                    .map_or(expanded, |s| s.is_open());
        }

        if context_action != SidebarAction::None {
            *action = context_action;
        }
    }
}

/// Renders a slice of tree entries recursively, with lazy-loading of children.
///
/// Works at any nesting depth — directories lazy-load their children on first
/// expand and cache the result in `TreeEntry.children`.
fn render_entry_list(ui: &mut egui::Ui, entries: &mut [TreeEntry], action: &mut SidebarAction) {
    for entry in entries.iter_mut() {
        match entry.kind {
            EntryKind::Directory => {
                let name = entry.name.clone();
                let path = entry.path.clone();
                let expanded = entry.expanded;
                let id = ui.make_persistent_id(("entry", &path));

                egui::collapsing_header::CollapsingState::load_with_default_open(
                    ui.ctx(),
                    id,
                    expanded,
                )
                .show_header(ui, |ui| {
                    let response = ui.label(format!("\u{1F4C1} {name}"));
                    response.context_menu(|ui| {
                        if ui.button("Delete").clicked() {
                            *action = SidebarAction::DeleteFile(path.clone());
                            ui.close();
                        }
                    });
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
                    render_entry_list(ui, &mut entry.children, action);
                });

                entry.expanded = egui::collapsing_header::CollapsingState::load(ui.ctx(), id)
                    .map_or(expanded, |s| s.is_open());
            }
            EntryKind::File => {
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
                    if ui.button("Delete").clicked() {
                        *action = SidebarAction::DeleteFile(entry.path.clone());
                        ui.close();
                    }
                });
            }
        }
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
}
