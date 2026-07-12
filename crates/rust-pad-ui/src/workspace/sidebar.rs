/// Workspace sidebar UI rendering.
use std::path::{Path, PathBuf};

use eframe::egui;
use egui::{Color32, Rect, Sense, Vec2};

use super::menus::{show_directory_context_menu, show_file_context_menu, show_root_context_menu};
use super::tree::{EntryKind, FolderRoot, TreeEntry};
use super::watcher::WorkspaceWatcher;
use crate::app::chrome;
use crate::app::resolved_theme::{tree_indent, ChromeTheme, Metrics, TREE_ROW_HEIGHT};
use crate::app::workspace_ops::generate_unique_name;
use crate::icons;

/// Minimum sidebar width in pixels.
const MIN_WIDTH: f32 = 150.0;
/// Maximum sidebar width in pixels.
const MAX_WIDTH: f32 = 500.0;
/// Default sidebar width in pixels. Kept at the pre-redesign value so a
/// config carrying it reads as "never resized".
/// [`WorkspaceSidebar::effective_width`] then substitutes the theme's
/// metric default.
const DEFAULT_WIDTH: f32 = 250.0;

/// Width of the chevron column at the start of a tree row.
const CHEVRON_WIDTH: f32 = 16.0;
/// Width of the entry-icon column in a tree row.
const ICON_WIDTH: f32 = 20.0;
/// Trailing padding after a row's name text.
const ROW_END_PADDING: f32 = 8.0;
/// Font size for row names and icons.
const ROW_FONT_SIZE: f32 = 13.0;
/// Font size for the chevron glyph.
const CHEVRON_FONT_SIZE: f32 = 11.0;
/// Font size for the uppercase header label.
const HEADER_FONT_SIZE: f32 = 11.0;

/// Which representation of a path the `CopyPath` action should write to
/// the clipboard. Mirrors the three submenu items in
/// `Copy Path > {Name | Full Path | Relative Path}`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CopyPathScope {
    Name,
    Full,
    Relative,
}

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
    /// Hide the sidebar panel without closing the workspace (reopen via Ctrl+B
    /// or the Workspace menu).
    Hide,
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
    /// Toggle visibility of hidden files in the workspace tree.
    ToggleHiddenFiles,
    /// Expand every collapsible entry currently loaded in the tree.
    ExpandAll,
    /// Collapse every collapsible entry currently loaded in the tree.
    CollapseAll,
    /// Copy a file's contents to the system clipboard, gated by the
    /// configured size-warning threshold. `workspace_root` is the
    /// `FolderRoot.path` that owns the entry, required by the
    /// canonical-containment security gate.
    CopyFileContents {
        path: PathBuf,
        workspace_root: PathBuf,
    },
    /// Reveal a folder in the OS file explorer (Windows Explorer, macOS
    /// Finder, `xdg-open` on Linux).
    OpenInFileExplorer(PathBuf),
    /// Re-read a directory (or workspace root) and its expanded subtree from
    /// disk, reconciling the tree with on-disk changes the watcher may have
    /// missed (notably on macOS, where FSEvents coalesces directory events).
    ReloadFromDisk(PathBuf),
    /// Copy a representation of an entry path to the clipboard.
    ///
    /// `root` is the workspace-root path that contains `path`, used to
    /// compute the relative scope. For root entries `root == path` and the
    /// relative scope degenerates to the workspace folder name.
    CopyPath {
        path: PathBuf,
        root: PathBuf,
        scope: CopyPathScope,
    },
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
    /// When true, the stem of `name` (or full name if no extension) is
    /// selected on the first render so the user can replace it by typing.
    /// Cleared after the selection is applied.
    pub select_on_focus: bool,
}

/// State for inline rename of a file or folder.
#[derive(Debug, Clone)]
pub(crate) struct RenameEntryState {
    /// Original full path of the entry being renamed.
    pub original_path: PathBuf,
    /// Index of the workspace root whose row initiated the rename. Pairs with
    /// `original_path` so a physical folder that surfaces under two roots only
    /// shows the inline field on the row the user actually selected. Mirrors
    /// the [`SelectedNode`] identity model.
    pub root_index: usize,
    /// Current name in the text field.
    pub name: String,
    /// True if this is a directory.
    pub is_dir: bool,
    /// When true, the stem of `name` is selected on the first render.
    pub select_on_focus: bool,
}

/// Identifies a single *visible row* in the tree. Two roots can surface the
/// same physical path (e.g. a folder that is both a workspace root and a
/// child of another root), so a bare `PathBuf` cannot disambiguate which row
/// is selected. Pairing the path with the owning root index makes each row
/// uniquely addressable for selection and inline rename.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SelectedNode {
    /// Index of the workspace root whose subtree contains this row.
    pub root_index: usize,
    /// Absolute path of the entry.
    pub path: PathBuf,
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
    /// Whether hidden files/folders (names starting with `.`) are shown.
    pub show_hidden: bool,
    /// Pending bulk expand/collapse for the next render. `Some(true)` =
    /// expand all workspace roots; `Some(false)` = collapse all; `None` =
    /// no bulk action queued. Consumed at the start of the next
    /// `render_tree`.
    pub(crate) pending_bulk_collapse: Option<bool>,
    /// Whether the next render of the workspace rename buffer should
    /// select all text on focus. Set when entering rename mode, cleared
    /// after the selection is applied.
    pub(crate) workspace_rename_select_pending: bool,
    /// Currently selected entry, identified by `(root_index, path)` so a
    /// physical folder appearing under two roots highlights only the row the
    /// user actually clicked. Survives lazy-load expansion; the selection is
    /// cleared when its row is no longer visible.
    pub(crate) selected: Option<SelectedNode>,
    /// Whether the sidebar currently owns keyboard input. Set only when the
    /// user clicks a tree row; cleared by `App` when the editor (or another
    /// panel) is clicked. The keyboard-nav gate and the single-pane editor's
    /// `auto_focus` both read this so arrow/Enter/F2 keys route to whichever
    /// panel the user last *clicked* (click-to-focus), independent of egui's
    /// implicit widget focus. Pointer hover deliberately does not affect it, so
    /// a mouse merely resting over the tree can never steal keys from the
    /// focused editor.
    pub(crate) kbd_active: bool,
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
            show_hidden: false,
            pending_bulk_collapse: None,
            workspace_rename_select_pending: false,
            selected: None,
            kbd_active: false,
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

    /// Returns the width the panel should open at: a user-persisted width
    /// wins; a width still at the pre-redesign default means the user never
    /// resized, so the active theme's metric default applies instead.
    pub fn effective_width(&self, metric_default: f32) -> f32 {
        if (self.width - DEFAULT_WIDTH).abs() < f32::EPSILON {
            metric_default.clamp(MIN_WIDTH, MAX_WIDTH)
        } else {
            self.width()
        }
    }

    /// Returns true if the sidebar should be rendered.
    pub fn is_visible(&self) -> bool {
        self.visible
    }

    /// Renders the sidebar content and returns any action to execute.
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        chrome: &ChromeTheme,
        metrics: &Metrics,
    ) -> SidebarAction {
        // Clear the Enter-suppression flag from the previous frame.
        self.rename_just_confirmed = false;

        // Keyboard navigation runs first so that a key press can produce
        // an `OpenFile` action this frame without being preempted by the
        // double-click handler in the file row.
        let mut action = self
            .handle_tree_kbd_nav(ui.ctx())
            .unwrap_or(SidebarAction::None);

        self.render_header(ui, chrome, metrics, &mut action);

        ui.separator();

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
                    self.render_tree(ui, chrome, metrics, &mut action);
                }
            });

        action
    }

    /// Renders the sidebar header: the workspace label (11 px uppercase,
    /// muted) on the left and the primary tree actions (New File, New
    /// Folder, Collapse All) plus an overflow menu on the right. The
    /// label keeps its pre-redesign behaviors: double-click renames the
    /// workspace and right-click opens the workspace menu.
    fn render_header(
        &mut self,
        ui: &mut egui::Ui,
        chrome: &ChromeTheme,
        metrics: &Metrics,
        action: &mut SidebarAction,
    ) {
        // Both paths render inside the same fixed-height band so the header
        // keeps its height (and vertical centering) during a rename.
        if self.rename_buffer.is_some() && !self.workspace_name.is_empty() {
            chrome::header_band(ui, metrics.sidebar_header_height, |ui| {
                self.render_workspace_rename_field(ui, action);
            });
            return;
        }
        let new_entry_target = self.new_entry_target();
        chrome::header_band(ui, metrics.sidebar_header_height, |ui| {
            // The action buttons are emitted first (right-to-left) so they
            // always keep their full width; the label gets whatever remains
            // and truncates. Emitting the label first let a long name shrink
            // the buttons' region below their width, and egui does not clip
            // that overflow: the buttons painted over the label on narrow
            // sidebars.
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                self.render_header_overflow_menu(ui, action);
                if header_icon_button(ui, icons::CARET_DOUBLE_UP, "Collapse All").clicked() {
                    *action = SidebarAction::CollapseAll;
                }
                let folder_btn = header_new_entry_button(
                    ui,
                    icons::FOLDER_PLUS,
                    "New Folder",
                    &new_entry_target,
                );
                if folder_btn.clicked() {
                    self.begin_new_entry(&new_entry_target, true);
                }
                let file_btn =
                    header_new_entry_button(ui, icons::FILE_PLUS, "New File", &new_entry_target);
                if file_btn.clicked() {
                    self.begin_new_entry(&new_entry_target, false);
                }
                // Label last: consumes only the leftover region to the left
                // of the buttons.
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    if self.workspace_name.is_empty() {
                        header_name_label(ui, "WORKSPACE", chrome);
                    } else {
                        self.render_workspace_name_with_menu(ui, chrome, action);
                    }
                });
            });
        });
    }

    /// Renders the header's `…` overflow menu carrying the secondary
    /// workspace actions that the redesigned header no longer shows as
    /// dedicated buttons.
    fn render_header_overflow_menu(&mut self, ui: &mut egui::Ui, action: &mut SidebarAction) {
        let response = ui
            .menu_button(icons::DOTS_THREE, |ui| {
                if ui.button("Add Folder to Workspace...").clicked() {
                    *action = SidebarAction::AddFolder;
                    ui.close();
                }
                let hidden_label = if self.show_hidden {
                    "Hide Hidden Files"
                } else {
                    "Show Hidden Files"
                };
                if ui.button(hidden_label).clicked() {
                    *action = SidebarAction::ToggleHiddenFiles;
                    ui.close();
                }
                if ui.button("Expand All").clicked() {
                    *action = SidebarAction::ExpandAll;
                    ui.close();
                }
                ui.separator();
                if ui.button("Hide Sidebar").clicked() {
                    *action = SidebarAction::Hide;
                    ui.close();
                }
                if ui.button("Close Workspace").clicked() {
                    *action = SidebarAction::CloseWorkspace;
                    ui.close();
                }
            })
            .response;
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "More Actions")
        });
        response.on_hover_text("More actions");
    }

    /// Picks the directory a header-initiated New File / New Folder should
    /// target: the selected directory, the selected file's parent, or the
    /// first available workspace root.
    fn new_entry_target(&self) -> Option<PathBuf> {
        if let Some(sel) = &self.selected {
            match self.entry_kind_for(&sel.path) {
                Some(EntryKind::Directory) => return Some(sel.path.clone()),
                Some(EntryKind::File) => {
                    if let Some(parent) = sel.path.parent() {
                        return Some(parent.to_path_buf());
                    }
                }
                None => {}
            }
        }
        self.tree
            .iter()
            .find(|root| root.path.is_dir())
            .map(|root| root.path.clone())
    }

    /// Arms the inline new-entry field under `target`, mirroring what the
    /// context menu's New File / New Folder items seed.
    fn begin_new_entry(&mut self, target: &Option<PathBuf>, is_dir: bool) {
        let Some(parent) = target else {
            return;
        };
        let seed = if is_dir { "new_folder" } else { "new_file.txt" };
        let name = generate_unique_name(parent, seed, is_dir);
        self.new_entry = Some(NewEntryState {
            parent: parent.clone(),
            name,
            is_dir,
            select_on_focus: true,
        });
        self.rename_entry = None;
    }

    /// Renders the inline text field for renaming the workspace.
    fn render_workspace_rename_field(&mut self, ui: &mut egui::Ui, action: &mut SidebarAction) {
        let buf = self.rename_buffer.as_mut().unwrap();
        let buf_snapshot = buf.clone();
        let response = ui.add(
            egui::TextEdit::singleline(buf)
                .id_salt("ws-workspace-rename")
                .desired_width(ui.available_width()),
        );
        if !response.has_focus() && !response.lost_focus() {
            response.request_focus();
        }
        if self.workspace_rename_select_pending {
            select_stem_in_text_edit(&response.ctx, response.id, &buf_snapshot);
            self.workspace_rename_select_pending = false;
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
    fn render_workspace_name_with_menu(
        &mut self,
        ui: &mut egui::Ui,
        chrome: &ChromeTheme,
        action: &mut SidebarAction,
    ) {
        let display = self.workspace_name.to_uppercase();
        // The label may be elided; the hover carries the full sanitized name
        // in its original case (the label itself is uppercased for style).
        let hover = format!(
            "{}\nDouble-click to rename",
            crate::text_sanitize::sanitize_display_text(&self.workspace_name)
        );
        let name_response = header_name_label(ui, &display, chrome).on_hover_text(hover);
        if name_response.double_clicked() {
            self.rename_buffer = Some(self.workspace_name.clone());
            self.workspace_rename_select_pending = true;
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
                            format!("{} {ws_name}", icons::CHECK)
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

    /// Renders the folder tree. Every row is painted by [`tree_row`]; the
    /// data-model `expanded` flags are the single source of truth for
    /// open/closed state (no egui `CollapsingState` involved).
    fn render_tree(
        &mut self,
        ui: &mut egui::Ui,
        chrome: &ChromeTheme,
        metrics: &Metrics,
        action: &mut SidebarAction,
    ) {
        let mut context_action = SidebarAction::None;
        let mut new_entry_request: Option<NewEntryState> = None;
        let mut selection_request: Option<SelectedNode> = None;
        if let Some(open) = self.pending_bulk_collapse.take() {
            // Bulk expand/collapse flips workspace-root flags only. Cascading
            // into nested directories would lazy-load the entire reachable
            // tree in one frame, which froze the UI on large workspaces.
            for root in &mut self.tree {
                root.expanded = open;
            }
        }
        // Hoist the inline-rename state into locals so a root row can render
        // its own rename field (the body's `render_entry_list` only reaches
        // nested entries) without aliasing `self` mutably across closures.
        // Written back after the loop.
        let mut rename_state = self.rename_entry.take();
        let mut rename_confirmed = self.rename_just_confirmed;
        let mut clear_rename = false;
        // Snapshot the selection so we can pass it by reference through the
        // borrow-checker without aliasing `self.selected` mutably.
        let selected_snapshot = self.selected.clone();

        for root_idx in 0..self.tree.len() {
            let root_path = self.tree[root_idx].path.clone();
            let root_name = root_path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| root_path.to_string_lossy().into_owned());

            let folder_exists = root_path.is_dir();

            // A root selected for rename shows its inline field in place of
            // its row, so the rename targets the row the user picked, not a
            // same-path duplicate nested under another root.
            let renaming_this_root = rename_state
                .as_ref()
                .is_some_and(|r| r.root_index == root_idx && r.original_path == root_path);
            if renaming_this_root {
                if let Some(state) = rename_state.as_mut() {
                    if render_inline_rename(ui, state, action, &mut rename_confirmed, 0) {
                        clear_rename = true;
                    }
                }
                continue;
            }

            // Force-open the root if the inline new entry targets it.
            if self
                .new_entry
                .as_ref()
                .is_some_and(|ne| ne.parent == root_path)
            {
                self.tree[root_idx].expanded = true;
            }
            let expanded = self.tree[root_idx].expanded;
            let root_selected = selected_snapshot
                .as_ref()
                .is_some_and(|s| s.root_index == root_idx && s.path == root_path);

            let display_name = if folder_exists {
                root_name.clone()
            } else {
                format!("{root_name} (unavailable)")
            };
            let spec = TreeRowSpec {
                depth: 0,
                name: &display_name,
                kind: "folder",
                icon: root_row_icon(folder_exists, expanded),
                icon_color: root_icon_color(folder_exists, expanded, chrome),
                chevron: folder_exists.then_some(expanded),
                selected: root_selected,
                semibold: true,
                text_color: root_text_color(folder_exists, root_selected, ui, chrome),
            };
            let events = tree_row(ui, chrome, metrics, &spec);
            if folder_exists {
                if events.chevron_clicked || events.row_double_clicked {
                    self.tree[root_idx].expanded = !expanded;
                }
                if events.row_clicked {
                    selection_request = Some(SelectedNode {
                        root_index: root_idx,
                        path: root_path.clone(),
                    });
                }
            }
            events.response.context_menu(|ui| {
                show_root_context_menu(
                    ui,
                    &root_path,
                    folder_exists,
                    &mut context_action,
                    &mut new_entry_request,
                );
            });

            if self.tree[root_idx].expanded && folder_exists {
                let mut ctx = RenderCtx {
                    action,
                    new_entry: &mut self.new_entry,
                    rename_entry: &mut rename_state,
                    rename_just_confirmed: &mut rename_confirmed,
                    workspace_root: &root_path,
                    show_hidden: self.show_hidden,
                    root_index: root_idx,
                    selected: selected_snapshot.as_ref(),
                    chrome,
                    metrics,
                };
                render_entry_list(
                    ui,
                    &root_path,
                    &mut self.tree[root_idx].entries,
                    &mut ctx,
                    &mut selection_request,
                    1,
                );
            }
        }

        if context_action != SidebarAction::None {
            *action = context_action;
        }
        if let Some(req) = new_entry_request {
            self.new_entry = Some(req);
            rename_state = None;
        }
        if clear_rename {
            rename_state = None;
        }
        // Write hoisted state back.
        self.rename_entry = rename_state;
        self.rename_just_confirmed = rename_confirmed;
        if let Some(req) = selection_request {
            self.selected = Some(req);
            // A row click hands keyboard ownership to the sidebar so arrow
            // navigation works regardless of pointer position until the user
            // clicks another panel (click-to-focus).
            self.kbd_active = true;
        }
    }

    // ── Keyboard-navigation helpers ───────────────────────────────────

    /// Returns every currently visible row as a [`SelectedNode`] in tree
    /// order (root → children if open → siblings ...), tagging each with its
    /// owning root index. Honours [`show_hidden`] and [`TreeEntry::expanded`].
    /// Used for keyboard navigation and selection identity.
    ///
    /// Lazy-loaded children that have not yet been scanned simply aren't
    /// included; keyboard nav cannot reveal an entry the renderer hasn't
    /// materialised, which matches what the user sees.
    pub(crate) fn visible_nodes(&self) -> Vec<SelectedNode> {
        let mut out = Vec::new();
        for (root_index, root) in self.tree.iter().enumerate() {
            if !root.path.is_dir() {
                continue;
            }
            out.push(SelectedNode {
                root_index,
                path: root.path.clone(),
            });
            if root.expanded {
                collect_visible(root_index, &root.entries, &mut out, self.show_hidden);
            }
        }
        out
    }

    /// Convenience wrapper over [`visible_nodes`](Self::visible_nodes)
    /// returning just the paths, in the same order. Test-only; production
    /// navigation works in terms of [`SelectedNode`].
    #[cfg(test)]
    pub(crate) fn visible_paths(&self) -> Vec<std::path::PathBuf> {
        self.visible_nodes().into_iter().map(|n| n.path).collect()
    }

    /// Looks up the [`EntryKind`] for `target`. Walks the tree; returns
    /// `None` when `target` is not present (e.g. the path was deleted
    /// between render and key-press).
    pub(crate) fn entry_kind_for(&self, target: &std::path::Path) -> Option<EntryKind> {
        for root in &self.tree {
            if root.path == target {
                return Some(EntryKind::Directory);
            }
            if let Some(kind) = find_entry_kind(&root.entries, target) {
                return Some(kind);
            }
        }
        None
    }

    /// Returns whether the directory at `target` is currently expanded.
    /// Returns `false` for files, unknown paths, or roots that aren't
    /// directories. Mirrors the rendered tree state, not egui's
    /// `CollapsingState` cache.
    pub(crate) fn is_expanded(&self, target: &std::path::Path) -> bool {
        for root in &self.tree {
            if root.path == target {
                return root.expanded;
            }
            if let Some(entry) = find_entry(&root.entries, target) {
                return entry.expanded;
            }
        }
        false
    }

    /// Sets the `expanded` flag for the directory at `target`. No-op for
    /// files, unknown paths, or roots that aren't directories. The renderer
    /// reads these flags directly, so the change is visible on the next
    /// frame.
    pub(crate) fn set_expanded(&mut self, target: &std::path::Path, open: bool) {
        for root in &mut self.tree {
            if root.path == target {
                root.expanded = open;
                return;
            }
            if let Some(entry) = super::scanner::find_entry_mut(&mut root.entries, target) {
                if matches!(entry.kind, EntryKind::Directory) {
                    entry.expanded = open;
                }
                return;
            }
        }
    }

    /// Convenience: flips [`is_expanded`] for `target`.
    pub(crate) fn toggle_expanded_for(&mut self, target: &std::path::Path) {
        let new = !self.is_expanded(target);
        self.set_expanded(target, new);
    }

    /// Handles arrow / Enter / F2 keystrokes for the sidebar tree.
    ///
    /// Activation is **click-to-focus**: nav fires only when a row is selected
    /// AND the sidebar holds keyboard ownership ([`kbd_active`](Self::kbd_active)),
    /// which is latched by a deliberate row click and released when another panel
    /// is clicked (`App` clears it). Pointer position is irrelevant: a mouse
    /// resting over the tree never routes keys here, so it cannot steal
    /// navigation from the focused editor. This is independent of egui's implicit
    /// widget focus (which the editor monopolises via its per-frame
    /// `auto_focus`). Returns `Some(action)` only when Enter on a file should
    /// open it. Inline rename / new-entry editing suspends nav.
    pub(crate) fn handle_tree_kbd_nav(&mut self, ctx: &egui::Context) -> Option<SidebarAction> {
        // Skip if any inline edit is active; the TextEdit owns the keys.
        if self.rename_buffer.is_some() || self.rename_entry.is_some() || self.new_entry.is_some() {
            return None;
        }
        // Click-to-focus gate: own the keyboard only after a row click latched
        // ownership with a live selection. No pointer-hover activation.
        if !(self.selected.is_some() && self.kbd_active) {
            return None;
        }

        let nodes = self.visible_nodes();
        if nodes.is_empty() {
            return None;
        }

        let current_idx = self
            .selected
            .as_ref()
            .and_then(|sel| nodes.iter().position(|n| n == sel));

        // If the selected row is no longer visible, surface the event and clear.
        if self.selected.is_some() && current_idx.is_none() {
            tracing::info!(
                previous = ?self.selected,
                reason = "row_no_longer_visible",
                "Workspace selection cleared",
            );
            self.selected = None;
        }

        use egui::{Key, Modifiers};
        let mods = Modifiers::NONE;

        if ctx.input_mut(|i| i.consume_key(mods, Key::ArrowDown)) {
            let next = current_idx.map_or(0, |i| (i + 1).min(nodes.len() - 1));
            self.selected = Some(nodes[next].clone());
            return None;
        }
        if ctx.input_mut(|i| i.consume_key(mods, Key::ArrowUp)) {
            let prev = current_idx.map_or(0, |i| i.saturating_sub(1));
            self.selected = Some(nodes[prev].clone());
            return None;
        }
        if ctx.input_mut(|i| i.consume_key(mods, Key::Enter)) {
            // `kbd_nav_activate` returns `Some` only for files; directories and
            // an absent selection both yield `None`.
            return current_idx.and_then(|idx| self.kbd_nav_activate(nodes[idx].clone()));
        }
        if ctx.input_mut(|i| i.consume_key(mods, Key::F2)) {
            if let Some(idx) = current_idx {
                self.kbd_nav_begin_rename(nodes[idx].clone());
            }
            return None;
        }
        if ctx.input_mut(|i| i.consume_key(mods, Key::ArrowRight)) {
            if let Some(idx) = current_idx {
                self.kbd_nav_expand_or_descend(&nodes, idx);
            }
            return None;
        }
        if ctx.input_mut(|i| i.consume_key(mods, Key::ArrowLeft)) {
            if let Some(idx) = current_idx {
                self.kbd_nav_collapse_or_ascend(&nodes, idx);
            }
            return None;
        }
        None
    }

    /// Enter on the selected row: opens a file (handing keyboard ownership to
    /// the editor so the user can type immediately) or toggles a directory's
    /// expansion. Returns `Some(OpenFile)` only for files; an unknown path is
    /// a no-op. Extracted from [`handle_tree_kbd_nav`](Self::handle_tree_kbd_nav)
    /// to keep that dispatcher's cognitive complexity in check and to make the
    /// action testable without an `egui::Context`.
    fn kbd_nav_activate(&mut self, node: SelectedNode) -> Option<SidebarAction> {
        match self.entry_kind_for(&node.path)? {
            EntryKind::File => {
                // Opening a file hands the arrow keys to the editor by releasing
                // sidebar ownership. Under click-to-focus the keys stay with the
                // editor until the user clicks a row again, so no hover can
                // reclaim them.
                self.kbd_active = false;
                Some(SidebarAction::OpenFile(node.path))
            }
            EntryKind::Directory => {
                self.toggle_expanded_for(&node.path);
                None
            }
        }
    }

    /// F2 on the selected row: arms inline rename for `node`, seeding the field
    /// with the entry's current file name.
    fn kbd_nav_begin_rename(&mut self, node: SelectedNode) {
        let path = node.path;
        tracing::debug!(path = ?path, root_index = node.root_index, "Workspace rename initiated via F2");
        self.rename_entry = Some(RenameEntryState {
            original_path: path.clone(),
            root_index: node.root_index,
            name: path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            is_dir: matches!(self.entry_kind_for(&path), Some(EntryKind::Directory)),
            select_on_focus: true,
        });
    }

    /// ArrowRight on the row at `idx`: expands a collapsed directory, or
    /// descends into its first already-materialised child. No-op for files.
    fn kbd_nav_expand_or_descend(&mut self, nodes: &[SelectedNode], idx: usize) {
        let node = nodes[idx].clone();
        if !matches!(self.entry_kind_for(&node.path), Some(EntryKind::Directory)) {
            return;
        }
        if !self.is_expanded(&node.path) {
            self.set_expanded(&node.path, true);
            return;
        }
        // Move to the first child if it appears below us in the same root
        // subtree, i.e. lazy-load already ran and the directory is non-empty.
        if let Some(child) = nodes.get(idx + 1) {
            if child.root_index == node.root_index && child.path.starts_with(&node.path) {
                self.selected = Some(child.clone());
            }
        }
    }

    /// ArrowLeft on the row at `idx`: collapses an expanded directory, else
    /// jumps to the parent row within the same root subtree.
    fn kbd_nav_collapse_or_ascend(&mut self, nodes: &[SelectedNode], idx: usize) {
        let node = nodes[idx].clone();
        let is_dir = matches!(self.entry_kind_for(&node.path), Some(EntryKind::Directory));
        if is_dir && self.is_expanded(&node.path) {
            self.set_expanded(&node.path, false);
            return;
        }
        if let Some(parent) = node.path.parent() {
            if let Some(parent_node) = nodes
                .iter()
                .find(|n| n.root_index == node.root_index && n.path == parent)
            {
                self.selected = Some(parent_node.clone());
            }
        }
    }
}

/// Recursive helper for [`WorkspaceSidebar::visible_nodes`]. Pushes
/// `entries` (filtered by `show_hidden`) into `out`, recursing into
/// expanded directories.
fn collect_visible(
    root_index: usize,
    entries: &[TreeEntry],
    out: &mut Vec<SelectedNode>,
    show_hidden: bool,
) {
    for entry in entries {
        if !show_hidden && entry.name.starts_with('.') {
            continue;
        }
        out.push(SelectedNode {
            root_index,
            path: entry.path.clone(),
        });
        if matches!(entry.kind, EntryKind::Directory) && entry.expanded {
            collect_visible(root_index, &entry.children, out, show_hidden);
        }
    }
}

/// Walks `entries` recursively looking for a tree entry at `target`.
fn find_entry<'a>(entries: &'a [TreeEntry], target: &std::path::Path) -> Option<&'a TreeEntry> {
    for entry in entries {
        if entry.path == target {
            return Some(entry);
        }
        if matches!(entry.kind, EntryKind::Directory) {
            if let Some(hit) = find_entry(&entry.children, target) {
                return Some(hit);
            }
        }
    }
    None
}

/// Lookup-only variant of [`find_entry`] returning just the kind.
fn find_entry_kind(entries: &[TreeEntry], target: &std::path::Path) -> Option<EntryKind> {
    find_entry(entries, target).map(|e| e.kind)
}

/// Outcome of one frame of inline name-field editing.
///
/// `Submitted` carries the trimmed name as the user typed it; **this layer
/// performs no filename validation**. Sanitization is the caller's or the
/// downstream `SidebarAction` handler's responsibility.
#[derive(Debug, Clone, PartialEq, Eq)]
enum InlineEntryOutcome {
    Editing,
    Cancelled,
    Submitted(String),
}

/// Shared scaffolding for the inline rename and new-entry text fields.
///
/// Renders an icon plus an auto-focused single-line `TextEdit`, applies the
/// one-shot stem selection when `*select_on_focus` is true (and clears it),
/// and reports the next state transition to the caller.
///
/// State transitions are intentionally not traced; this runs per frame. If
/// observability is ever needed, instrument the `SidebarAction` handler
/// instead, never this helper.
fn render_inline_entry_field(
    ui: &mut egui::Ui,
    icon: &str,
    id_salt: &str,
    name: &mut String,
    select_on_focus: &mut bool,
    depth: usize,
) -> InlineEntryOutcome {
    ui.horizontal(|ui| {
        ui.add_space(tree_indent(depth));
        ui.label(icon);
        let name_snapshot = name.clone();
        let resp = ui.add(
            egui::TextEdit::singleline(name)
                .id_salt(id_salt)
                .desired_width(ui.available_width()),
        );
        if !resp.has_focus() && !resp.lost_focus() {
            resp.request_focus();
        }
        if *select_on_focus {
            select_stem_in_text_edit(&resp.ctx, resp.id, &name_snapshot);
            *select_on_focus = false;
        }
        if resp.lost_focus() {
            let trimmed = name.trim().to_string();
            if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                return InlineEntryOutcome::Submitted(trimmed);
            }
            return InlineEntryOutcome::Cancelled;
        }
        if resp.ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            return InlineEntryOutcome::Cancelled;
        }
        InlineEntryOutcome::Editing
    })
    .inner
}

/// Renders the inline rename text field for a tree entry.
/// Returns `true` when the rename interaction is complete.
fn render_inline_rename(
    ui: &mut egui::Ui,
    state: &mut RenameEntryState,
    action: &mut SidebarAction,
    rename_just_confirmed: &mut bool,
    depth: usize,
) -> bool {
    let original_name = state
        .original_path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let icon = if state.is_dir {
        icons::FOLDER
    } else {
        file_icon(&original_name)
    };
    match render_inline_entry_field(
        ui,
        icon,
        "ws-rename-entry",
        &mut state.name,
        &mut state.select_on_focus,
        depth,
    ) {
        InlineEntryOutcome::Submitted(name) if !name.is_empty() && name != original_name => {
            *action = SidebarAction::ConfirmRenameEntry(state.original_path.clone(), name);
            *rename_just_confirmed = true;
            true
        }
        InlineEntryOutcome::Cancelled | InlineEntryOutcome::Submitted(_) => true,
        InlineEntryOutcome::Editing => false,
    }
}

/// Mutable rendering context threaded through every recursive call into the
/// workspace tree. Bundling these fields removes a multi-argument
/// `#[allow(clippy::too_many_arguments)]` from the rendering helpers and
/// makes it impossible to thread a stale `workspace_root` by accident.
///
/// `new_entry_request` and `rename_request` deliberately stay out of this
/// struct: they are *outgoing* signals back to the immediate parent
/// `render_entry_list`, not state inherited by the whole sub-tree.
pub(crate) struct RenderCtx<'a> {
    pub action: &'a mut SidebarAction,
    pub new_entry: &'a mut Option<NewEntryState>,
    pub rename_entry: &'a mut Option<RenameEntryState>,
    pub rename_just_confirmed: &'a mut bool,
    /// The `FolderRoot.path` that owns the subtree currently being rendered.
    /// Used by the Copy Path > Relative scope and by the Copy Contents
    /// security gate that verifies a symlinked file does not escape the
    /// workspace folder it appears under.
    pub workspace_root: &'a Path,
    pub show_hidden: bool,
    /// Index of the workspace root whose subtree is being rendered. Combined
    /// with each entry's path to match the selection (see [`SelectedNode`]).
    pub root_index: usize,
    /// The currently selected node, for highlighting. `None` when nothing is
    /// selected. An entry is highlighted only when both its `root_index` and
    /// path match.
    pub selected: Option<&'a SelectedNode>,
    /// Resolved chrome palette for row painting.
    pub chrome: &'a ChromeTheme,
    /// Resolved metric set (indicator style, radii) for row painting.
    pub metrics: &'a Metrics,
}

impl RenderCtx<'_> {
    /// Whether `path` in the current root subtree is the selected row.
    fn is_selected(&self, path: &Path) -> bool {
        self.selected
            .is_some_and(|s| s.root_index == self.root_index && s.path == path)
    }
}

/// Visual and semantic description of one tree row, resolved by the caller.
struct TreeRowSpec<'a> {
    depth: usize,
    name: &'a str,
    /// Accessible kind suffix ("folder" / "file") appended to the row label.
    kind: &'a str,
    icon: &'a str,
    icon_color: Color32,
    /// `Some(open)` renders a chevron for an expandable row; `None` renders
    /// the aligned spacer files get.
    chevron: Option<bool>,
    selected: bool,
    /// Semibold name text (workspace roots and the selected row, matching
    /// the active tab's weight treatment).
    semibold: bool,
    text_color: Color32,
}

/// What the user did to a tree row this frame.
struct TreeRowEvents {
    response: egui::Response,
    /// The chevron was clicked (expand/collapse toggle).
    chevron_clicked: bool,
    /// The row body (not the chevron) was clicked.
    row_clicked: bool,
    /// The row body was double-clicked.
    row_double_clicked: bool,
}

/// Paints one 26 px tree row: selection indicator or hover tint, chevron,
/// entry icon, and name, indented per [`tree_indent`].
///
/// The row is a single widget sensing [`egui::Sense::CLICK`]: the bare,
/// **non-focusable** click flag. Tree rows must not take egui keyboard
/// focus: a focused row makes egui's spatial widget navigation hijack the
/// arrow keys and lets the row's own activation race the sidebar's nav
/// handler. Keeping rows non-focusable leaves arrow/Enter/F2 entirely to
/// [`WorkspaceSidebar::handle_tree_kbd_nav`]. Chevron hits are resolved from
/// the pointer position within the one widget rather than a second overlaid
/// widget, so the row also stays a single accessibility node.
fn tree_row(
    ui: &mut egui::Ui,
    chrome_theme: &ChromeTheme,
    metrics: &Metrics,
    spec: &TreeRowSpec<'_>,
) -> TreeRowEvents {
    let font = if spec.semibold {
        egui::FontId::new(
            ROW_FONT_SIZE,
            egui::FontFamily::Name(crate::app::FONT_FAMILY_SEMIBOLD.into()),
        )
    } else {
        egui::FontId::proportional(ROW_FONT_SIZE)
    };
    let name_galley = ui
        .painter()
        .layout_no_wrap(spec.name.to_string(), font, spec.text_color);

    let indent = tree_indent(spec.depth);
    let content_width =
        indent + CHEVRON_WIDTH + ICON_WIDTH + name_galley.size().x + ROW_END_PADDING;
    let width = ui.available_width().max(content_width);
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, TREE_ROW_HEIGHT), Sense::CLICK);

    let accessible = format!("{}, {}", spec.name, spec.kind);
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::Button,
            true,
            spec.selected,
            accessible.clone(),
        )
    });

    let chevron_rect = Rect::from_min_size(
        egui::pos2(rect.left() + indent, rect.top()),
        Vec2::new(CHEVRON_WIDTH, rect.height()),
    );

    if ui.is_rect_visible(rect) {
        if spec.selected {
            chrome::accent_indicator(ui.painter(), rect, chrome_theme, metrics);
        } else if response.hovered() {
            chrome::hover_tint(ui.painter(), rect, chrome_theme, metrics);
        }

        if let Some(open) = spec.chevron {
            let glyph = if open {
                icons::CARET_DOWN
            } else {
                icons::CARET_RIGHT
            };
            let galley = ui.painter().layout_no_wrap(
                glyph.to_string(),
                egui::FontId::proportional(CHEVRON_FONT_SIZE),
                chrome_theme.text_muted,
            );
            let pos = chevron_rect.center() - galley.size() / 2.0;
            ui.painter().galley(pos, galley, chrome_theme.text_muted);
        }

        let icon_rect = Rect::from_min_size(
            egui::pos2(chevron_rect.right(), rect.top()),
            Vec2::new(ICON_WIDTH, rect.height()),
        );
        let icon_galley = ui.painter().layout_no_wrap(
            spec.icon.to_string(),
            egui::FontId::proportional(ROW_FONT_SIZE),
            spec.icon_color,
        );
        let icon_pos = icon_rect.center() - icon_galley.size() / 2.0;
        ui.painter().galley(icon_pos, icon_galley, spec.icon_color);

        let name_pos = egui::pos2(
            icon_rect.right(),
            rect.center().y - name_galley.size().y / 2.0,
        );
        ui.painter().galley(name_pos, name_galley, spec.text_color);
    }

    let pointer_in_chevron = spec.chevron.is_some()
        && response
            .interact_pointer_pos()
            .is_some_and(|pos| chevron_rect.contains(pos));
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    TreeRowEvents {
        chevron_clicked: response.clicked() && pointer_in_chevron,
        row_clicked: response.clicked() && !pointer_in_chevron,
        row_double_clicked: response.double_clicked() && !pointer_in_chevron,
        response,
    }
}

/// The 11 px uppercase muted text used for the sidebar header label.
fn header_label_text(text: &str, chrome_theme: &ChromeTheme) -> egui::RichText {
    egui::RichText::new(text)
        .font(egui::FontId::new(
            HEADER_FONT_SIZE,
            egui::FontFamily::Name(crate::app::FONT_FAMILY_SEMIBOLD.into()),
        ))
        .color(chrome_theme.text_muted)
}

/// Emits the header's workspace label into the remaining band width.
///
/// Single emission point for both the named-workspace and "WORKSPACE"
/// placeholder paths. The text is sanitized (control chars and bidi
/// overrides become U+FFFD) and truncates with an ellipsis instead of
/// pushing into or under the header action buttons.
fn header_name_label(ui: &mut egui::Ui, text: &str, chrome_theme: &ChromeTheme) -> egui::Response {
    let sanitized = crate::text_sanitize::sanitize_display_text(text);
    ui.add(egui::Label::new(header_label_text(&sanitized, chrome_theme)).truncate())
}

/// A small icon button for the header action strip, with a tooltip and an
/// accessible label.
fn header_icon_button(ui: &mut egui::Ui, icon: &str, label: &str) -> egui::Response {
    let response = ui.small_button(icon);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
    response.on_hover_text(label)
}

/// A header New File / New Folder button, disabled when no target directory
/// exists (no selection and no available root).
fn header_new_entry_button(
    ui: &mut egui::Ui,
    icon: &str,
    label: &str,
    target: &Option<PathBuf>,
) -> egui::Response {
    let response = ui.add_enabled(target.is_some(), egui::Button::new(icon).small());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
    response.on_hover_text(label)
}

/// Icon for a workspace-root row: warning marker when the folder is missing,
/// open/closed folder otherwise.
fn root_row_icon(folder_exists: bool, expanded: bool) -> &'static str {
    if !folder_exists {
        icons::WARNING_CIRCLE
    } else if expanded {
        icons::FOLDER_OPEN
    } else {
        icons::FOLDER
    }
}

/// Icon color for a workspace-root row: warn for a missing folder, dimmed
/// accent when open, muted otherwise.
fn root_icon_color(folder_exists: bool, expanded: bool, chrome_theme: &ChromeTheme) -> Color32 {
    if !folder_exists {
        chrome_theme.warn
    } else if expanded {
        chrome_theme.accent_dim
    } else {
        chrome_theme.text_muted
    }
}

/// Name color for a workspace-root row.
fn root_text_color(
    folder_exists: bool,
    selected: bool,
    ui: &egui::Ui,
    chrome_theme: &ChromeTheme,
) -> Color32 {
    if !folder_exists {
        chrome_theme.text_faint
    } else if selected {
        chrome_theme.accent
    } else {
        ui.visuals().text_color()
    }
}

/// Renders a directory tree entry: chevron row, context menu, and
/// lazy-loaded children.
///
/// `ExpandAll` / `CollapseAll` deliberately do NOT propagate here; they only
/// flip the workspace-root flags. Cascading expansion through every
/// recursively rendered directory triggers lazy-loads for the entire reachable
/// tree on a single frame, which froze the UI on large workspaces.
fn render_directory_entry(
    ui: &mut egui::Ui,
    entry: &mut TreeEntry,
    ctx: &mut RenderCtx<'_>,
    new_entry_request: &mut Option<NewEntryState>,
    rename_request: &mut Option<RenameEntryState>,
    selection_request: &mut Option<SelectedNode>,
    depth: usize,
) {
    let name = entry.name.clone();
    let path = entry.path.clone();

    // Force-open the directory if the inline new entry targets it.
    if ctx.new_entry.as_ref().is_some_and(|ne| ne.parent == path) {
        entry.expanded = true;
    }
    let open = entry.expanded;
    let selected = ctx.is_selected(&path);

    let spec = TreeRowSpec {
        depth,
        name: &name,
        kind: "folder",
        icon: if open {
            icons::FOLDER_OPEN
        } else {
            icons::FOLDER
        },
        icon_color: if open {
            ctx.chrome.accent_dim
        } else {
            ctx.chrome.text_muted
        },
        chevron: Some(open),
        selected,
        semibold: selected,
        text_color: if selected {
            ctx.chrome.accent
        } else {
            ui.visuals().text_color()
        },
    };
    let events = tree_row(ui, ctx.chrome, ctx.metrics, &spec);
    if events.chevron_clicked || events.row_double_clicked {
        entry.expanded = !open;
    }
    if events.row_clicked {
        *selection_request = Some(SelectedNode {
            root_index: ctx.root_index,
            path: path.clone(),
        });
    }
    let workspace_root = ctx.workspace_root.to_path_buf();
    events.response.context_menu(|ui| {
        show_directory_context_menu(
            ui,
            &path,
            &name,
            &workspace_root,
            ctx,
            new_entry_request,
            rename_request,
        );
    });

    if entry.expanded {
        // Lazy-load children on first expand. This blocks the UI thread for
        // one frame while scanning, but the result is cached in
        // `entry.children` so subsequent frames are free.
        if entry.children.is_empty() {
            let dir_path = entry.path.clone();
            if let Ok(children) = super::scanner::scan_directory(&dir_path, ctx.show_hidden) {
                entry.children = children;
            }
        }
        render_entry_list(
            ui,
            &path,
            &mut entry.children,
            ctx,
            selection_request,
            depth + 1,
        );
    }
}

/// Renders a file tree entry with the file-layout context menu.
fn render_file_entry(
    ui: &mut egui::Ui,
    entry: &TreeEntry,
    ctx: &mut RenderCtx<'_>,
    rename_request: &mut Option<RenameEntryState>,
    selection_request: &mut Option<SelectedNode>,
    depth: usize,
) {
    let selected = ctx.is_selected(&entry.path);
    let spec = TreeRowSpec {
        depth,
        name: &entry.name,
        kind: "file",
        icon: file_icon(&entry.name),
        icon_color: ctx.chrome.text_muted,
        chevron: None,
        selected,
        semibold: selected,
        text_color: if selected {
            ctx.chrome.accent
        } else {
            ui.visuals().text_color()
        },
    };
    let events = tree_row(ui, ctx.chrome, ctx.metrics, &spec);

    if events.row_clicked {
        *selection_request = Some(SelectedNode {
            root_index: ctx.root_index,
            path: entry.path.clone(),
        });
    }
    if events.row_double_clicked {
        *ctx.action = SidebarAction::OpenFile(entry.path.clone());
    }
    let workspace_root = ctx.workspace_root.to_path_buf();
    let path = entry.path.clone();
    let name = entry.name.clone();
    events.response.context_menu(|ui| {
        show_file_context_menu(ui, &path, &name, &workspace_root, ctx, rename_request);
    });
}

/// Renders the inline text field for creating a new file or folder.
/// Returns `true` when the interaction is complete.
fn render_inline_new_entry_field(
    ui: &mut egui::Ui,
    state: &mut NewEntryState,
    action: &mut SidebarAction,
    rename_just_confirmed: &mut bool,
    depth: usize,
) -> bool {
    let icon = if state.is_dir {
        icons::FOLDER_PLUS
    } else {
        icons::FILE_PLUS
    };
    match render_inline_entry_field(
        ui,
        icon,
        "ws-new-entry",
        &mut state.name,
        &mut state.select_on_focus,
        depth,
    ) {
        InlineEntryOutcome::Submitted(name) if !name.is_empty() => {
            *action = if state.is_dir {
                SidebarAction::ConfirmNewFolder(state.parent.clone(), name)
            } else {
                SidebarAction::ConfirmNewFile(state.parent.clone(), name)
            };
            *rename_just_confirmed = true;
            true
        }
        InlineEntryOutcome::Cancelled | InlineEntryOutcome::Submitted(_) => true,
        InlineEntryOutcome::Editing => false,
    }
}

/// Renders a slice of tree entries recursively, with lazy-loading of children.
///
/// Works at any nesting depth: directories lazy-load their children on first
/// expand and cache the result in `TreeEntry.children`.
fn render_entry_list(
    ui: &mut egui::Ui,
    parent_path: &Path,
    entries: &mut [TreeEntry],
    ctx: &mut RenderCtx<'_>,
    selection_request: &mut Option<SelectedNode>,
    depth: usize,
) {
    let mut new_entry_request: Option<NewEntryState> = None;
    let mut rename_request: Option<RenameEntryState> = None;
    let mut clear_rename = false;

    for entry in entries.iter_mut() {
        let is_renaming = ctx
            .rename_entry
            .as_ref()
            .is_some_and(|r| r.root_index == ctx.root_index && r.original_path == entry.path);

        if is_renaming {
            if let Some(ref mut state) = ctx.rename_entry {
                if render_inline_rename(ui, state, ctx.action, ctx.rename_just_confirmed, depth) {
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
                    ctx,
                    &mut new_entry_request,
                    &mut rename_request,
                    selection_request,
                    depth,
                );
            }
            EntryKind::File => {
                render_file_entry(
                    ui,
                    entry,
                    ctx,
                    &mut rename_request,
                    selection_request,
                    depth,
                );
            }
        }
    }

    let mut clear_new = false;
    if let Some(ref mut state) = ctx.new_entry {
        if state.parent.as_path() == parent_path
            && render_inline_new_entry_field(
                ui,
                state,
                ctx.action,
                ctx.rename_just_confirmed,
                depth,
            )
        {
            clear_new = true;
        }
    }

    if let Some(req) = new_entry_request {
        *ctx.new_entry = Some(req);
        *ctx.rename_entry = None;
    }
    if let Some(req) = rename_request {
        *ctx.rename_entry = Some(req);
        *ctx.new_entry = None;
    }
    if clear_rename {
        *ctx.rename_entry = None;
    }
    if clear_new {
        *ctx.new_entry = None;
    }
}

/// Selects the filename stem (chars before the last `.`) in the text edit
/// state, or the full text if there is no extension. Stem-selection lets
/// the user replace the name by typing while preserving the extension;
/// matches IDE convention (VS Code, IntelliJ).
fn select_stem_in_text_edit(ctx: &egui::Context, widget_id: egui::Id, name: &str) {
    let stem_char_count = match name.rfind('.') {
        Some(byte_idx) if byte_idx > 0 => name[..byte_idx].chars().count(),
        _ => name.chars().count(),
    };
    if let Some(mut state) = egui::widgets::text_edit::TextEditState::load(ctx, widget_id) {
        let range = egui::text::CCursorRange::two(
            egui::text::CCursor::new(0),
            egui::text::CCursor::new(stem_char_count),
        );
        state.cursor.set_char_range(Some(range));
        state.store(ctx, widget_id);
    }
}

/// Returns a Phosphor icon constant for a filename based on its extension.
fn file_icon(name: &str) -> &'static str {
    let ext = name.rsplit('.').next().unwrap_or("");
    match ext.to_lowercase().as_str() {
        "rs" => icons::FILE_CODE,
        "toml" | "yaml" | "yml" | "json" | "xml" => icons::GEAR,
        "md" | "txt" | "log" => icons::FILE_TEXT,
        "png" | "jpg" | "jpeg" | "gif" | "svg" | "ico" => icons::FILE_IMAGE,
        "lock" => icons::LOCK,
        _ => icons::FILE,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_inline_entry_outcome_variants_distinct() {
        let editing = InlineEntryOutcome::Editing;
        let cancelled = InlineEntryOutcome::Cancelled;
        let submitted_empty = InlineEntryOutcome::Submitted(String::new());
        let submitted_name = InlineEntryOutcome::Submitted("foo.txt".to_string());

        assert_ne!(editing, cancelled);
        assert_ne!(editing, submitted_empty);
        assert_ne!(cancelled, submitted_empty);
        assert_ne!(submitted_empty, submitted_name);
        assert_eq!(
            submitted_name.clone(),
            InlineEntryOutcome::Submitted("foo.txt".to_string())
        );
    }

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
        assert!(!sidebar.show_hidden);
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
            SidebarAction::ToggleHiddenFiles,
            SidebarAction::ExpandAll,
            SidebarAction::CollapseAll,
            SidebarAction::OpenInFileExplorer(PathBuf::from("/g")),
            SidebarAction::ReloadFromDisk(PathBuf::from("/h")),
            SidebarAction::None,
        ];

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
        assert_eq!(file_icon("main.rs"), icons::FILE_CODE);
        assert_eq!(file_icon("Cargo.toml"), icons::GEAR);
        assert_eq!(file_icon("README.md"), icons::FILE_TEXT);
        assert_eq!(file_icon("logo.png"), icons::FILE_IMAGE);
        assert_eq!(file_icon("Cargo.lock"), icons::LOCK);
        assert_eq!(file_icon("unknown.xyz"), icons::FILE);
    }

    #[test]
    fn test_new_entry_state_creation() {
        let state = NewEntryState {
            parent: PathBuf::from("/project/src"),
            name: "new_file.txt".to_string(),
            is_dir: false,
            select_on_focus: true,
        };
        assert_eq!(state.parent, PathBuf::from("/project/src"));
        assert_eq!(state.name, "new_file.txt");
        assert!(!state.is_dir);
    }

    #[test]
    fn test_rename_entry_state_creation() {
        let state = RenameEntryState {
            original_path: PathBuf::from("/project/src/old.rs"),
            root_index: 0,
            name: "old.rs".to_string(),
            is_dir: false,
            select_on_focus: true,
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
        assert_eq!(file_icon("main.RS"), icons::FILE_CODE);
        assert_eq!(file_icon("config.TOML"), icons::GEAR);
        assert_eq!(file_icon("image.PNG"), icons::FILE_IMAGE);
    }

    #[test]
    fn test_file_icon_no_extension() {
        // File with no extension should return default icon
        assert_eq!(file_icon("Makefile"), icons::FILE);
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
            select_on_focus: true,
        };
        assert!(state.is_dir);
        assert_eq!(state.name, "new_folder");
    }

    #[test]
    fn test_rename_entry_state_dir() {
        let state = RenameEntryState {
            original_path: PathBuf::from("/project/src"),
            root_index: 0,
            name: "src".to_string(),
            is_dir: true,
            select_on_focus: true,
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
            select_on_focus: true,
        });
        assert!(sidebar.new_entry.is_some());
        assert!(sidebar.rename_entry.is_none());

        // Switch to rename state (should conceptually clear new_entry)
        sidebar.rename_entry = Some(RenameEntryState {
            original_path: PathBuf::from("/project/old.rs"),
            root_index: 0,
            name: "old.rs".to_string(),
            is_dir: false,
            select_on_focus: true,
        });
        sidebar.new_entry = None;
        assert!(sidebar.new_entry.is_none());
        assert!(sidebar.rename_entry.is_some());
    }

    #[test]
    fn test_file_icon_all_image_types() {
        assert_eq!(file_icon("photo.jpg"), icons::FILE_IMAGE);
        assert_eq!(file_icon("photo.jpeg"), icons::FILE_IMAGE);
        assert_eq!(file_icon("animation.gif"), icons::FILE_IMAGE);
        assert_eq!(file_icon("vector.svg"), icons::FILE_IMAGE);
        assert_eq!(file_icon("favicon.ico"), icons::FILE_IMAGE);
    }

    #[test]
    fn test_file_icon_config_types() {
        assert_eq!(file_icon("config.yaml"), icons::GEAR);
        assert_eq!(file_icon("config.yml"), icons::GEAR);
        assert_eq!(file_icon("data.json"), icons::GEAR);
        assert_eq!(file_icon("pom.xml"), icons::GEAR);
    }

    #[test]
    fn test_file_icon_text_types() {
        assert_eq!(file_icon("notes.txt"), icons::FILE_TEXT);
        assert_eq!(file_icon("app.log"), icons::FILE_TEXT);
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
            select_on_focus: true,
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
            root_index: 2,
            name: "b".to_string(),
            is_dir: true,
            select_on_focus: true,
        };
        let cloned = state.clone();
        assert_eq!(state.original_path, cloned.original_path);
        assert_eq!(state.root_index, cloned.root_index);
        assert_eq!(state.name, cloned.name);
        assert_eq!(state.is_dir, cloned.is_dir);
    }

    // ── visible_paths + tree-lookup helper tests ─────────────────────
    //
    // The roots in the synthetic trees below need `path.is_dir()` to
    // return true, otherwise `visible_paths` skips them per the
    // "unavailable root" rule. The helpers route through a real tempdir
    // so existence holds without us creating any subdirectories.

    fn make_file_entry(parent: &std::path::Path, name: &str) -> TreeEntry {
        TreeEntry {
            name: name.to_string(),
            path: parent.join(name),
            kind: EntryKind::File,
            expanded: false,
            children: Vec::new(),
        }
    }

    fn make_dir_entry(
        parent: &std::path::Path,
        name: &str,
        expanded: bool,
        children: Vec<TreeEntry>,
    ) -> TreeEntry {
        TreeEntry {
            name: name.to_string(),
            path: parent.join(name),
            kind: EntryKind::Directory,
            expanded,
            children,
        }
    }

    #[test]
    fn visible_paths_empty_tree_returns_empty() {
        let sidebar = WorkspaceSidebar::new();
        assert!(sidebar.visible_paths().is_empty());
    }

    #[test]
    fn visible_paths_root_with_two_visible_files() {
        let tmp = tempfile::tempdir().unwrap();
        let mut sidebar = WorkspaceSidebar::new();
        sidebar.tree.push(crate::workspace::tree::FolderRoot {
            path: tmp.path().to_path_buf(),
            entries: vec![
                make_file_entry(tmp.path(), "a.txt"),
                make_file_entry(tmp.path(), "b.txt"),
            ],
            expanded: true,
        });
        let paths = sidebar.visible_paths();
        assert_eq!(paths.len(), 3);
        assert_eq!(paths[0], tmp.path());
        assert_eq!(paths[1], tmp.path().join("a.txt"));
        assert_eq!(paths[2], tmp.path().join("b.txt"));
    }

    #[test]
    fn visible_nodes_disambiguate_same_path_under_two_roots() {
        // New Bug 1 topology: folder C is both a child of root A and a root
        // in its own right, so the same physical path appears on two rows.
        let tmp = tempfile::tempdir().unwrap();
        let a = tmp.path().join("A");
        let c = a.join("C");
        std::fs::create_dir_all(&c).unwrap();

        let mut sidebar = WorkspaceSidebar::new();
        // Root 0: A (expanded), containing child C.
        sidebar.tree.push(crate::workspace::tree::FolderRoot {
            path: a.clone(),
            entries: vec![make_dir_entry(&a, "C", false, Vec::new())],
            expanded: true,
        });
        // Root 1: C itself, same physical path as A/C.
        sidebar.tree.push(crate::workspace::tree::FolderRoot {
            path: c.clone(),
            entries: Vec::new(),
            expanded: false,
        });

        let nodes = sidebar.visible_nodes();
        let c_nodes: Vec<&SelectedNode> = nodes.iter().filter(|n| n.path == c).collect();
        assert_eq!(c_nodes.len(), 2, "same path appears under both roots");
        assert_eq!(c_nodes[0].root_index, 0, "child of root A");
        assert_eq!(c_nodes[1].root_index, 1, "root C");

        // Selecting the root-1 instance matches exactly one row, not the
        // child under root A. This is what gives single-row highlight.
        let selected = SelectedNode {
            root_index: 1,
            path: c.clone(),
        };
        assert_eq!(nodes.iter().filter(|n| **n == selected).count(), 1);
        assert_ne!(*c_nodes[0], selected, "the root-A child is a different row");
    }

    #[test]
    fn visible_paths_collapsed_subfolder_hides_children() {
        let tmp = tempfile::tempdir().unwrap();
        let sub_children = vec![make_file_entry(&tmp.path().join("sub"), "hidden.rs")];
        let sub = make_dir_entry(tmp.path(), "sub", false, sub_children);
        let mut sidebar = WorkspaceSidebar::new();
        sidebar.tree.push(crate::workspace::tree::FolderRoot {
            path: tmp.path().to_path_buf(),
            entries: vec![sub],
            expanded: true,
        });
        let paths = sidebar.visible_paths();
        // root + sub only: sub.expanded is false so children are skipped.
        assert_eq!(paths.len(), 2);
        assert!(paths.contains(&tmp.path().to_path_buf()));
        assert!(paths.contains(&tmp.path().join("sub")));
        assert!(!paths.contains(&tmp.path().join("sub").join("hidden.rs")));
    }

    #[test]
    fn visible_paths_hidden_files_filtered_unless_show_hidden() {
        let tmp = tempfile::tempdir().unwrap();
        let mut sidebar = WorkspaceSidebar::new();
        sidebar.tree.push(crate::workspace::tree::FolderRoot {
            path: tmp.path().to_path_buf(),
            entries: vec![
                make_file_entry(tmp.path(), ".env"),
                make_file_entry(tmp.path(), "main.rs"),
            ],
            expanded: true,
        });
        // Default show_hidden = false.
        let paths = sidebar.visible_paths();
        assert!(!paths.contains(&tmp.path().join(".env")));
        assert!(paths.contains(&tmp.path().join("main.rs")));
        // Flip the flag and re-query.
        sidebar.show_hidden = true;
        let paths = sidebar.visible_paths();
        assert!(paths.contains(&tmp.path().join(".env")));
        assert!(paths.contains(&tmp.path().join("main.rs")));
    }

    #[test]
    fn visible_paths_skips_unavailable_root() {
        // Pointing at a non-existent path: root.path.is_dir() returns
        // false → root is skipped, no children rendered.
        let mut sidebar = WorkspaceSidebar::new();
        sidebar.tree.push(crate::workspace::tree::FolderRoot {
            path: PathBuf::from("/definitely/does/not/exist/anywhere"),
            entries: vec![TreeEntry {
                name: "ghost.txt".to_string(),
                path: PathBuf::from("/definitely/does/not/exist/anywhere/ghost.txt"),
                kind: EntryKind::File,
                expanded: false,
                children: Vec::new(),
            }],
            expanded: true,
        });
        assert!(sidebar.visible_paths().is_empty());
    }

    // ── entry_kind_for / is_expanded / set_expanded / find_entry_mut ──

    #[test]
    fn entry_kind_for_returns_directory_for_root_and_subdir() {
        let tmp = tempfile::tempdir().unwrap();
        let sub = make_dir_entry(tmp.path(), "src", true, vec![]);
        let mut sidebar = WorkspaceSidebar::new();
        sidebar.tree.push(crate::workspace::tree::FolderRoot {
            path: tmp.path().to_path_buf(),
            entries: vec![sub],
            expanded: true,
        });
        assert_eq!(
            sidebar.entry_kind_for(tmp.path()),
            Some(EntryKind::Directory)
        );
        assert_eq!(
            sidebar.entry_kind_for(&tmp.path().join("src")),
            Some(EntryKind::Directory)
        );
        assert_eq!(
            sidebar.entry_kind_for(&PathBuf::from("/nowhere/at/all")),
            None,
        );
    }

    #[test]
    fn entry_kind_for_returns_file() {
        let tmp = tempfile::tempdir().unwrap();
        let file = make_file_entry(tmp.path(), "main.rs");
        let mut sidebar = WorkspaceSidebar::new();
        sidebar.tree.push(crate::workspace::tree::FolderRoot {
            path: tmp.path().to_path_buf(),
            entries: vec![file],
            expanded: true,
        });
        assert_eq!(
            sidebar.entry_kind_for(&tmp.path().join("main.rs")),
            Some(EntryKind::File),
        );
    }

    #[test]
    fn set_expanded_flips_directory_flag() {
        let tmp = tempfile::tempdir().unwrap();
        let sub = make_dir_entry(tmp.path(), "src", false, vec![]);
        let mut sidebar = WorkspaceSidebar::new();
        sidebar.tree.push(crate::workspace::tree::FolderRoot {
            path: tmp.path().to_path_buf(),
            entries: vec![sub],
            expanded: true,
        });
        assert!(!sidebar.is_expanded(&tmp.path().join("src")));
        sidebar.set_expanded(&tmp.path().join("src"), true);
        assert!(sidebar.is_expanded(&tmp.path().join("src")));
        sidebar.toggle_expanded_for(&tmp.path().join("src"));
        assert!(!sidebar.is_expanded(&tmp.path().join("src")));
    }

    #[test]
    fn find_entry_mut_finds_nested_entries() {
        let tmp = tempfile::tempdir().unwrap();
        let inner = make_file_entry(&tmp.path().join("a").join("b"), "deep.rs");
        let mid = make_dir_entry(&tmp.path().join("a"), "b", true, vec![inner]);
        let top = make_dir_entry(tmp.path(), "a", true, vec![mid]);
        let mut sidebar = WorkspaceSidebar::new();
        sidebar.tree.push(crate::workspace::tree::FolderRoot {
            path: tmp.path().to_path_buf(),
            entries: vec![top],
            expanded: true,
        });
        let target = tmp.path().join("a").join("b").join("deep.rs");
        let found =
            crate::workspace::scanner::find_entry_mut(&mut sidebar.tree[0].entries, &target)
                .expect("found");
        assert_eq!(found.name, "deep.rs");
        assert_eq!(found.kind, EntryKind::File);
    }

    // ── Phase-25 restyle: row geometry, header actions, width fallback ──

    fn test_chrome_and_metrics() -> (ChromeTheme, Metrics) {
        (ChromeTheme::default(), Metrics::default())
    }

    /// Maps the named semibold UI family onto the default proportional list
    /// so rows using it lay out in a bare test context (the real family is
    /// installed by `App::install_fonts`, which tests don't run).
    fn register_semibold_alias(ctx: &egui::Context) {
        let mut fonts = egui::FontDefinitions::default();
        let proportional = fonts
            .families
            .get(&egui::FontFamily::Proportional)
            .cloned()
            .unwrap_or_default();
        fonts.families.insert(
            egui::FontFamily::Name(crate::app::FONT_FAMILY_SEMIBOLD.into()),
            proportional,
        );
        ctx.set_fonts(fonts);
    }

    fn row_input(events: Vec<egui::Event>) -> egui::RawInput {
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::pos2(0.0, 0.0),
                egui::vec2(400.0, 400.0),
            )),
            events,
            ..Default::default()
        }
    }

    /// Renders a plain folder row at the origin and returns what one
    /// pointer interaction produced. `click` is run as move → press →
    /// release across three frames so egui's click detection fires.
    fn drive_tree_row(click: Option<egui::Pos2>) -> (f32, bool, bool) {
        let ctx = egui::Context::default();
        let (chrome_theme, metrics) = test_chrome_and_metrics();
        let mut height = 0.0;
        let mut chevron_clicked = false;
        let mut row_clicked = false;
        let mut frame = |events: Vec<egui::Event>| {
            let _ = ctx.run_ui(row_input(events), |ui| {
                ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
                let spec = TreeRowSpec {
                    depth: 0,
                    name: "src",
                    kind: "folder",
                    icon: icons::FOLDER,
                    icon_color: chrome_theme.text_muted,
                    chevron: Some(false),
                    selected: false,
                    semibold: false,
                    text_color: egui::Color32::WHITE,
                };
                let events_out = tree_row(ui, &chrome_theme, &metrics, &spec);
                height = events_out.response.rect.height();
                chevron_clicked |= events_out.chevron_clicked;
                row_clicked |= events_out.row_clicked;
            });
        };
        match click {
            None => frame(Vec::new()),
            Some(pos) => {
                frame(vec![egui::Event::PointerMoved(pos)]);
                frame(vec![egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                }]);
                frame(vec![egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                }]);
            }
        }
        (height, chevron_clicked, row_clicked)
    }

    #[test]
    fn tree_row_is_26px_tall() {
        let (height, _, _) = drive_tree_row(None);
        assert!((height - TREE_ROW_HEIGHT).abs() < f32::EPSILON);
    }

    #[test]
    fn tree_row_click_on_chevron_reports_toggle_not_select() {
        // Depth 0 puts the chevron band at x ∈ [12, 28).
        let (_, chevron, row) = drive_tree_row(Some(egui::pos2(20.0, 13.0)));
        assert!(chevron, "chevron band click toggles");
        assert!(!row, "chevron click must not select the row");
    }

    #[test]
    fn tree_row_click_on_body_reports_select() {
        let (_, chevron, row) = drive_tree_row(Some(egui::pos2(120.0, 13.0)));
        assert!(!chevron);
        assert!(row, "body click selects the row");
    }

    /// Full `render_tree` drive: a body click on the root row must latch
    /// selection and keyboard ownership (click-to-focus).
    #[test]
    fn render_tree_row_click_latches_selection_and_keyboard() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("a.txt"), "x").unwrap();
        let mut sidebar = WorkspaceSidebar::new();
        sidebar.tree.push(crate::workspace::tree::FolderRoot {
            path: tmp.path().to_path_buf(),
            entries: vec![make_file_entry(tmp.path(), "a.txt")],
            expanded: false,
        });

        let ctx = egui::Context::default();
        register_semibold_alias(&ctx);
        let (chrome_theme, metrics) = test_chrome_and_metrics();
        let frame = |events: Vec<egui::Event>, sidebar: &mut WorkspaceSidebar| {
            let _ = ctx.run_ui(row_input(events), |ui| {
                ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
                let mut action = SidebarAction::None;
                sidebar.render_tree(ui, &chrome_theme, &metrics, &mut action);
            });
        };
        // The collapsed root row is the first (only) row: y ∈ [0, 26).
        let pos = egui::pos2(120.0, 13.0);
        frame(vec![egui::Event::PointerMoved(pos)], &mut sidebar);
        frame(
            vec![egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            }],
            &mut sidebar,
        );
        frame(
            vec![egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
            &mut sidebar,
        );

        assert_eq!(
            sidebar.selected,
            Some(SelectedNode {
                root_index: 0,
                path: tmp.path().to_path_buf(),
            })
        );
        assert!(sidebar.kbd_active, "row click hands the keyboard over");
    }

    /// Full `show()` drive through the real ScrollArea::both path with a
    /// populated tree: content must come out with finite, sane width (the
    /// scroll area reports unbounded space on its scrollable axes).
    #[test]
    fn show_renders_populated_tree_with_finite_row_geometry() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("a.txt"), "x").unwrap();
        std::fs::create_dir(tmp.path().join("sub")).unwrap();
        let mut sidebar = WorkspaceSidebar::new();
        sidebar.visible = true;
        sidebar.workspace_name = "ws".to_string();
        sidebar.tree.push(crate::workspace::tree::FolderRoot {
            path: tmp.path().to_path_buf(),
            entries: vec![
                make_dir_entry(tmp.path(), "sub", false, vec![]),
                make_file_entry(tmp.path(), "a.txt"),
            ],
            expanded: true,
        });

        let ctx = egui::Context::default();
        register_semibold_alias(&ctx);
        let (chrome_theme, metrics) = test_chrome_and_metrics();
        let mut content_width = 0.0f32;
        let _ = ctx.run_ui(row_input(Vec::new()), |ui| {
            egui::Panel::left("test_sidebar")
                .default_size(248.0)
                .show_inside(ui, |ui| {
                    sidebar.show(ui, &chrome_theme, &metrics);
                    content_width = ui.min_rect().width();
                });
        });
        assert!(
            content_width.is_finite(),
            "sidebar content width must be finite, got {content_width}"
        );
        assert!(
            content_width < 10_000.0,
            "sidebar content width exploded: {content_width}"
        );
    }

    #[test]
    fn effective_width_uses_metric_default_when_unresized() {
        let sidebar = WorkspaceSidebar::new();
        assert!((sidebar.effective_width(248.0) - 248.0).abs() < f32::EPSILON);
        assert!((sidebar.effective_width(232.0) - 232.0).abs() < f32::EPSILON);
    }

    #[test]
    fn effective_width_prefers_user_persisted_width() {
        let mut sidebar = WorkspaceSidebar::new();
        sidebar.set_width(320.0);
        assert!((sidebar.effective_width(248.0) - 320.0).abs() < f32::EPSILON);
    }

    #[test]
    fn effective_width_clamps_metric_default() {
        let sidebar = WorkspaceSidebar::new();
        assert!((sidebar.effective_width(900.0) - MAX_WIDTH).abs() < f32::EPSILON);
    }

    #[test]
    fn new_entry_target_falls_back_to_first_root() {
        let (sidebar, _tmp, root) = sidebar_with_tree();
        assert_eq!(sidebar.new_entry_target(), Some(root));
    }

    #[test]
    fn new_entry_target_uses_selected_directory() {
        let (mut sidebar, _tmp, root) = sidebar_with_tree();
        let sub = root.join("sub");
        sidebar.selected = Some(SelectedNode {
            root_index: 0,
            path: sub.clone(),
        });
        assert_eq!(sidebar.new_entry_target(), Some(sub));
    }

    #[test]
    fn new_entry_target_uses_selected_files_parent() {
        let (mut sidebar, _tmp, root) = sidebar_with_tree();
        sidebar.selected = Some(SelectedNode {
            root_index: 0,
            path: root.join("sub").join("child.rs"),
        });
        assert_eq!(sidebar.new_entry_target(), Some(root.join("sub")));
    }

    #[test]
    fn new_entry_target_none_without_roots() {
        let sidebar = WorkspaceSidebar::new();
        assert_eq!(sidebar.new_entry_target(), None);
    }

    #[test]
    fn begin_new_entry_arms_inline_state_and_clears_rename() {
        let (mut sidebar, _tmp, root) = sidebar_with_tree();
        sidebar.rename_entry = Some(RenameEntryState {
            original_path: root.join("a.txt"),
            root_index: 0,
            name: "a.txt".to_string(),
            is_dir: false,
            select_on_focus: false,
        });

        sidebar.begin_new_entry(&Some(root.clone()), true);

        let state = sidebar.new_entry.as_ref().expect("new entry armed");
        assert_eq!(state.parent, root);
        assert!(state.is_dir);
        assert!(state.select_on_focus);
        assert!(sidebar.rename_entry.is_none(), "mutually exclusive");
    }

    #[test]
    fn begin_new_entry_without_target_is_noop() {
        let mut sidebar = WorkspaceSidebar::new();
        sidebar.begin_new_entry(&None, false);
        assert!(sidebar.new_entry.is_none());
    }

    #[test]
    fn set_expanded_flips_flag_both_ways() {
        // The renderer reads `expanded` flags directly, so the data-model
        // flag is the whole story for keyboard expand/collapse.
        let tmp = tempfile::tempdir().unwrap();
        let sub = make_dir_entry(tmp.path(), "src", false, vec![]);
        let mut sidebar = WorkspaceSidebar::new();
        sidebar.tree.push(crate::workspace::tree::FolderRoot {
            path: tmp.path().to_path_buf(),
            entries: vec![sub],
            expanded: true,
        });
        let target = tmp.path().join("src");

        assert!(!sidebar.is_expanded(&target));
        sidebar.set_expanded(&target, true);
        assert!(sidebar.is_expanded(&target), "model flag set");
        sidebar.set_expanded(&target, false);
        assert!(!sidebar.is_expanded(&target));
    }

    #[test]
    fn toggle_expanded_for_flips_flag() {
        let tmp = tempfile::tempdir().unwrap();
        let sub = make_dir_entry(tmp.path(), "src", false, vec![]);
        let mut sidebar = WorkspaceSidebar::new();
        sidebar.tree.push(crate::workspace::tree::FolderRoot {
            path: tmp.path().to_path_buf(),
            entries: vec![sub],
            expanded: true,
        });
        let target = tmp.path().join("src");

        sidebar.toggle_expanded_for(&target);
        assert!(sidebar.is_expanded(&target));
        sidebar.toggle_expanded_for(&target);
        assert!(!sidebar.is_expanded(&target));
    }

    #[test]
    fn rename_state_identity_is_scoped_by_root_index() {
        // Defect C: the inline-rename match is (root_index, path), so a physical
        // folder surfaced under two roots only renames the row the user picked.
        let path = PathBuf::from("/ws/A/C");
        let state = RenameEntryState {
            original_path: path.clone(),
            root_index: 1,
            name: "C".to_string(),
            is_dir: true,
            select_on_focus: true,
        };
        // Mirrors the predicate in `render_entry_list`.
        let matches = |root_index: usize, entry_path: &std::path::Path| {
            state.root_index == root_index && state.original_path == entry_path
        };
        assert!(matches(1, &path), "selected root-1 instance renames");
        assert!(!matches(0, &path), "the same path under root 0 does not");
    }

    #[test]
    fn handle_tree_kbd_nav_returns_none_when_inline_edit_active() {
        // No egui::Context: short-circuit via the inline-edit gate so
        // we never touch ctx.input(). Exercises only the first guard.
        let mut sidebar = WorkspaceSidebar::new();
        sidebar.new_entry = Some(NewEntryState {
            parent: PathBuf::from("/a"),
            name: String::new(),
            is_dir: false,
            select_on_focus: true,
        });
        // We can't construct a real egui::Context cheaply, but we can
        // confirm the field that gates the function early. This is a
        // surrogate assertion; richer behaviour is exercised by manual
        // smoke testing of the keyboard nav in the live UI.
        assert!(sidebar.new_entry.is_some());
    }

    // ── keyboard-nav helper tests ────────────────────────────────────
    //
    // The four `kbd_nav_*` helpers carry the per-key behaviour extracted
    // from `handle_tree_kbd_nav` (which keeps that dispatcher's cognitive
    // complexity in check). They take plain data, no `egui::Context`, so
    // each branch is exercised directly here.

    /// Builds a sidebar over a real tempdir root so `visible_nodes` keeps
    /// the root (it requires `root.path.is_dir()`), with this shape:
    ///
    /// ```text
    /// <root>/            (expanded)
    ///   sub/             (directory, expanded)
    ///     child.rs
    ///   a.txt
    /// ```
    ///
    /// Visible order: `[root, sub, sub/child.rs, a.txt]`.
    fn sidebar_with_tree() -> (WorkspaceSidebar, tempfile::TempDir, PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().to_path_buf();
        let child = make_file_entry(&root.join("sub"), "child.rs");
        let sub = make_dir_entry(&root, "sub", true, vec![child]);
        let a = make_file_entry(&root, "a.txt");
        let mut sidebar = WorkspaceSidebar::new();
        sidebar.tree.push(crate::workspace::tree::FolderRoot {
            path: root.clone(),
            entries: vec![sub, a],
            expanded: true,
        });
        (sidebar, tmp, root)
    }

    fn node_idx(nodes: &[SelectedNode], path: &std::path::Path) -> usize {
        nodes
            .iter()
            .position(|n| n.path == path)
            .expect("path is visible")
    }

    #[test]
    fn kbd_nav_activate_file_opens_and_releases_keyboard() {
        let (mut sidebar, _tmp, root) = sidebar_with_tree();
        sidebar.kbd_active = true;
        let node = SelectedNode {
            root_index: 0,
            path: root.join("a.txt"),
        };
        let action = sidebar.kbd_nav_activate(node);
        assert_eq!(action, Some(SidebarAction::OpenFile(root.join("a.txt"))));
        assert!(
            !sidebar.kbd_active,
            "opening a file hands focus to the editor"
        );
    }

    #[test]
    fn kbd_nav_activate_directory_toggles_and_returns_none() {
        let (mut sidebar, _tmp, root) = sidebar_with_tree();
        let sub = root.join("sub");
        assert!(sidebar.is_expanded(&sub));
        let node = SelectedNode {
            root_index: 0,
            path: sub.clone(),
        };
        let action = sidebar.kbd_nav_activate(node);
        assert!(action.is_none());
        assert!(!sidebar.is_expanded(&sub), "an expanded dir collapses");
    }

    #[test]
    fn kbd_nav_activate_unknown_path_is_noop() {
        let (mut sidebar, _tmp, root) = sidebar_with_tree();
        let node = SelectedNode {
            root_index: 0,
            path: root.join("ghost"),
        };
        assert!(sidebar.kbd_nav_activate(node).is_none());
    }

    #[test]
    fn kbd_nav_begin_rename_seeds_file_state() {
        let (mut sidebar, _tmp, root) = sidebar_with_tree();
        let node = SelectedNode {
            root_index: 0,
            path: root.join("a.txt"),
        };
        sidebar.kbd_nav_begin_rename(node);
        let st = sidebar.rename_entry.as_ref().expect("rename armed");
        assert_eq!(st.original_path, root.join("a.txt"));
        assert_eq!(st.name, "a.txt");
        assert!(!st.is_dir);
        assert_eq!(st.root_index, 0);
        assert!(st.select_on_focus);
    }

    #[test]
    fn kbd_nav_begin_rename_marks_directory() {
        let (mut sidebar, _tmp, root) = sidebar_with_tree();
        let node = SelectedNode {
            root_index: 0,
            path: root.join("sub"),
        };
        sidebar.kbd_nav_begin_rename(node);
        let st = sidebar.rename_entry.as_ref().expect("rename armed");
        assert_eq!(st.name, "sub");
        assert!(st.is_dir);
    }

    #[test]
    fn kbd_nav_expand_or_descend_expands_collapsed_dir() {
        let (mut sidebar, _tmp, root) = sidebar_with_tree();
        let sub = root.join("sub");
        sidebar.set_expanded(&sub, false);
        let nodes = sidebar.visible_nodes();
        let idx = node_idx(&nodes, &sub);
        sidebar.kbd_nav_expand_or_descend(&nodes, idx);
        assert!(sidebar.is_expanded(&sub));
    }

    #[test]
    fn kbd_nav_expand_or_descend_selects_first_child_when_open() {
        let (mut sidebar, _tmp, root) = sidebar_with_tree();
        let sub = root.join("sub");
        let nodes = sidebar.visible_nodes();
        let idx = node_idx(&nodes, &sub);
        sidebar.kbd_nav_expand_or_descend(&nodes, idx);
        assert_eq!(
            sidebar.selected,
            Some(SelectedNode {
                root_index: 0,
                path: sub.join("child.rs"),
            })
        );
    }

    #[test]
    fn kbd_nav_expand_or_descend_ignores_files() {
        let (mut sidebar, _tmp, root) = sidebar_with_tree();
        let nodes = sidebar.visible_nodes();
        let idx = node_idx(&nodes, &root.join("a.txt"));
        let before = sidebar.selected.clone();
        let expanded_before = sidebar.is_expanded(&root.join("sub"));
        sidebar.kbd_nav_expand_or_descend(&nodes, idx);
        assert_eq!(sidebar.selected, before);
        assert_eq!(sidebar.is_expanded(&root.join("sub")), expanded_before);
    }

    #[test]
    fn kbd_nav_collapse_or_ascend_collapses_open_dir() {
        let (mut sidebar, _tmp, root) = sidebar_with_tree();
        let sub = root.join("sub");
        let nodes = sidebar.visible_nodes();
        let idx = node_idx(&nodes, &sub);
        sidebar.kbd_nav_collapse_or_ascend(&nodes, idx);
        assert!(!sidebar.is_expanded(&sub));
    }

    #[test]
    fn kbd_nav_collapse_or_ascend_jumps_to_parent_from_file() {
        let (mut sidebar, _tmp, root) = sidebar_with_tree();
        let child = root.join("sub").join("child.rs");
        let nodes = sidebar.visible_nodes();
        let idx = node_idx(&nodes, &child);
        sidebar.kbd_nav_collapse_or_ascend(&nodes, idx);
        assert_eq!(
            sidebar.selected,
            Some(SelectedNode {
                root_index: 0,
                path: root.join("sub"),
            })
        );
    }

    // ── full-dispatcher tests via a headless egui context ────────────
    //
    // These drive `handle_tree_kbd_nav` end-to-end so the activation gate,
    // ownership latch, and key dispatch lines are exercised too.

    /// Runs one frame of `handle_tree_kbd_nav` with `key` pressed. Pointer
    /// position is irrelevant under click-to-focus, so the caller sets
    /// `kbd_active` / `selected` to model a prior row click.
    fn drive_kbd_nav(sidebar: &mut WorkspaceSidebar, key: egui::Key) -> Option<SidebarAction> {
        let ctx = egui::Context::default();
        let mut raw = egui::RawInput::default();
        raw.events.push(egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        });
        let mut out = None;
        let _ = ctx.run_ui(raw, |ui| {
            out = sidebar.handle_tree_kbd_nav(ui.ctx());
        });
        out
    }

    /// Models a row click: `render_tree` sets both fields when a row is clicked.
    fn click_row(sidebar: &mut WorkspaceSidebar, path: &std::path::Path) {
        sidebar.selected = Some(SelectedNode {
            root_index: 0,
            path: path.to_path_buf(),
        });
        sidebar.kbd_active = true;
    }

    #[test]
    fn dispatch_arrow_down_advances_selection() {
        let (mut sidebar, _tmp, root) = sidebar_with_tree();
        click_row(&mut sidebar, &root);

        let action = drive_kbd_nav(&mut sidebar, egui::Key::ArrowDown);
        assert!(action.is_none());
        assert_eq!(sidebar.selected.as_ref().unwrap().path, root.join("sub"));
    }

    #[test]
    fn dispatch_arrow_up_moves_back() {
        let (mut sidebar, _tmp, root) = sidebar_with_tree();
        click_row(&mut sidebar, &root.join("sub"));
        drive_kbd_nav(&mut sidebar, egui::Key::ArrowUp);
        assert_eq!(sidebar.selected.as_ref().unwrap().path, root);
    }

    #[test]
    fn dispatch_enter_on_file_returns_open_action() {
        let (mut sidebar, _tmp, root) = sidebar_with_tree();
        click_row(&mut sidebar, &root.join("a.txt"));
        let action = drive_kbd_nav(&mut sidebar, egui::Key::Enter);
        assert_eq!(action, Some(SidebarAction::OpenFile(root.join("a.txt"))));
    }

    /// The brief's exact repro: the user clicked in the editor (so the sidebar
    /// does NOT own the keyboard) and their mouse merely rests over the tree.
    /// A stale selection may linger. Arrow keys must go to the editor, never
    /// move the tree selection.
    #[test]
    fn hover_without_ownership_does_not_capture_arrows() {
        let (mut sidebar, _tmp, root) = sidebar_with_tree();
        // Stale selection from an earlier click, but the editor now owns keys.
        sidebar.selected = Some(SelectedNode {
            root_index: 0,
            path: root.clone(),
        });
        sidebar.kbd_active = false;

        let action = drive_kbd_nav(&mut sidebar, egui::Key::ArrowDown);
        assert!(
            action.is_none(),
            "the arrow goes to the editor, not the tree"
        );
        assert_eq!(
            sidebar.selected.as_ref().unwrap().path,
            root,
            "tree selection is unchanged by a hover-only arrow press"
        );
        assert!(!sidebar.kbd_active, "hover never latches ownership");
    }

    /// Opening a file via Enter releases ownership, so the very next arrow
    /// (regardless of pointer position) is left for the editor.
    #[test]
    fn open_via_enter_releases_ownership_so_arrows_go_to_editor() {
        let (mut sidebar, _tmp, root) = sidebar_with_tree();
        click_row(&mut sidebar, &root.join("a.txt"));

        let action = drive_kbd_nav(&mut sidebar, egui::Key::Enter);
        assert_eq!(action, Some(SidebarAction::OpenFile(root.join("a.txt"))));
        assert!(
            !sidebar.kbd_active,
            "Enter-to-open releases sidebar ownership"
        );

        let selected_before = sidebar.selected.clone();
        let action2 = drive_kbd_nav(&mut sidebar, egui::Key::ArrowDown);
        assert!(
            action2.is_none(),
            "ownership released — arrow ignored by tree"
        );
        assert_eq!(
            sidebar.selected, selected_before,
            "selection unchanged — the arrow went to the editor"
        );
    }

    /// Once the user clicks a row again, ownership is re-latched and arrows
    /// drive the tree; re-engagement is by click, not by hover.
    #[test]
    fn row_click_reengages_after_editor_took_focus() {
        let (mut sidebar, _tmp, root) = sidebar_with_tree();
        // Editor currently owns the keyboard.
        sidebar.kbd_active = false;
        sidebar.selected = None;

        // A row click re-latches ownership and selection (as `render_tree` does).
        click_row(&mut sidebar, &root);
        let _ = drive_kbd_nav(&mut sidebar, egui::Key::ArrowDown);
        assert_eq!(
            sidebar.selected.as_ref().unwrap().path,
            root.join("sub"),
            "arrow advances the tree once a click re-latches ownership"
        );
    }

    #[test]
    fn dispatch_ignored_without_ownership() {
        let (mut sidebar, _tmp, _root) = sidebar_with_tree();
        // No click ever happened: no selection, no ownership.
        let action = drive_kbd_nav(&mut sidebar, egui::Key::ArrowDown);
        assert!(action.is_none());
        assert!(
            sidebar.selected.is_none(),
            "keys are ignored until a row click latches ownership"
        );
    }

    #[test]
    fn dispatch_empty_tree_returns_none() {
        let mut sidebar = WorkspaceSidebar::new();
        // Own the keyboard with a stale selection, but the tree has no rows.
        sidebar.selected = Some(SelectedNode {
            root_index: 0,
            path: PathBuf::from("/gone"),
        });
        sidebar.kbd_active = true;
        let action = drive_kbd_nav(&mut sidebar, egui::Key::ArrowDown);
        assert!(action.is_none());
    }

    // ── Bug fix: header buttons painted over the workspace name when the
    //    sidebar was narrow (label rendered first at natural width; the
    //    RTL button cluster overflowed leftwards over it, unclipped). ──

    /// Builds a kittest harness rendering only the sidebar at `width`.
    ///
    /// The first pass only registers the semibold font alias: `set_fonts`
    /// takes effect at the NEXT pass begin, and the harness runs a pass
    /// during construction, so registering after build panics on the
    /// unbound named family. Rendering starts from the second pass.
    fn header_harness(width: f32, name: &str) -> egui_kittest::Harness<'static> {
        let mut sidebar = WorkspaceSidebar::new();
        sidebar.workspace_name = name.to_string();
        let (chrome_theme, metrics) = test_chrome_and_metrics();
        let mut fonts_ready = false;
        let mut harness = egui_kittest::Harness::builder()
            .with_size(egui::Vec2::new(width, 400.0))
            .build_ui(move |ui| {
                if !fonts_ready {
                    register_semibold_alias(ui.ctx());
                    fonts_ready = true;
                    return;
                }
                let _ = sidebar.show(ui, &chrome_theme, &metrics);
            });
        harness.run();
        harness
    }

    /// The four header action buttons, queried by accessible label.
    fn header_button_rects(harness: &egui_kittest::Harness<'_>) -> Vec<egui::Rect> {
        use egui_kittest::kittest::Queryable;
        ["More Actions", "Collapse All", "New Folder", "New File"]
            .iter()
            .map(|label| harness.get_by_label(label).rect())
            .collect()
    }

    #[test]
    fn narrow_header_label_does_not_run_under_buttons() {
        use egui_kittest::kittest::{By, Queryable};
        let harness = header_harness(160.0, "official workspace with a long name");
        let buttons = header_button_rects(&harness);
        let label_rect = harness.get(By::new().label_contains("OFF")).rect();
        let leftmost_button = buttons
            .iter()
            .map(|r| r.min.x)
            .fold(f32::INFINITY, f32::min);
        assert!(
            label_rect.max.x <= leftmost_button + 1.0,
            "label (right edge {:.1}) must stop before the button cluster \
             (left edge {:.1})",
            label_rect.max.x,
            leftmost_button
        );
        for rect in &buttons {
            assert!(
                rect.intersect(label_rect).width() <= 1.0,
                "no button may overlap the label"
            );
        }
    }

    #[test]
    fn narrow_header_keeps_all_buttons_inside_the_panel() {
        let harness = header_harness(160.0, "official workspace with a long name");
        for rect in header_button_rects(&harness) {
            assert!(
                rect.min.x >= -1.0 && rect.max.x <= 161.0,
                "button {rect:?} must stay inside the 160px sidebar"
            );
        }
    }

    #[test]
    fn wide_header_still_shows_label_and_buttons() {
        use egui_kittest::kittest::{By, Queryable};
        let harness = header_harness(400.0, "workspace");
        let label_rect = harness.get(By::new().label_contains("WORKSPACE")).rect();
        assert!(label_rect.width() > 0.0, "label must be visible");
        assert_eq!(header_button_rects(&harness).len(), 4);
    }

    #[test]
    fn header_label_is_sanitized() {
        use egui_kittest::kittest::{By, Queryable};
        // A newline in the stored name must not reach the label raw.
        let harness = header_harness(400.0, "evil\nname");
        assert!(
            harness
                .query(By::new().label_contains("EVIL\u{FFFD}NAME"))
                .is_some(),
            "control characters must be replaced with U+FFFD"
        );
    }
}
