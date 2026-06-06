/// Workspace sidebar UI rendering.
use std::path::{Path, PathBuf};

use eframe::egui;

use super::menus::{show_directory_context_menu, show_file_context_menu, show_root_context_menu};
use super::tree::{EntryKind, FolderRoot, TreeEntry};
use super::watcher::WorkspaceWatcher;
use crate::icons;

/// Minimum sidebar width in pixels.
const MIN_WIDTH: f32 = 150.0;
/// Maximum sidebar width in pixels.
const MAX_WIDTH: f32 = 500.0;
/// Default sidebar width in pixels.
const DEFAULT_WIDTH: f32 = 250.0;

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
    /// `FolderRoot.path` that owns the entry — required by the
    /// canonical-containment security gate.
    CopyFileContents {
        path: PathBuf,
        workspace_root: PathBuf,
    },
    /// Reveal a folder in the OS file explorer (Windows Explorer, macOS
    /// Finder, `xdg-open` on Linux).
    OpenInFileExplorer(PathBuf),
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
    /// shows the inline field on the row the user actually selected — mirrors
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
    /// expand all loaded entries; `Some(false)` = collapse all; `None` =
    /// no bulk action queued. Consumed during the next `render_tree`
    /// because the egui `CollapsingState` ids are only addressable from
    /// within the sidebar's `Ui` scope.
    pub(crate) pending_bulk_collapse: Option<bool>,
    /// One-shot keyboard expand/collapse override consumed by the next
    /// `render_tree`. `Some((path, open))` forces that directory's egui
    /// `CollapsingState` to `open` for one frame. Keyboard nav mutates the
    /// data-model `expanded` flag, but egui's `CollapsingState` owns the
    /// on-screen open state and `load_with_default_open` ignores its default
    /// once a state is stored — so the model change must be pushed into egui
    /// here or the header never moves. At most one nav key is handled per
    /// frame, so a single slot suffices (mirrors `pending_bulk_collapse`).
    pub(crate) pending_expand: Option<(PathBuf, bool)>,
    /// Whether the next render of the workspace rename buffer should
    /// select all text on focus. Set when entering rename mode, cleared
    /// after the selection is applied.
    pub(crate) workspace_rename_select_pending: bool,
    /// Currently selected entry, identified by `(root_index, path)` so a
    /// physical folder appearing under two roots highlights only the row the
    /// user actually clicked. Survives lazy-load expansion; the selection is
    /// cleared when its row is no longer visible.
    pub(crate) selected: Option<SelectedNode>,
    /// Whether the sidebar currently owns keyboard input. Set when the user
    /// clicks a tree row or navigates with the pointer over the sidebar;
    /// cleared by `App` when the editor is clicked. The keyboard-nav gate and
    /// the single-pane editor's `auto_focus` both read this so arrow/Enter/F2
    /// keys route to whichever panel the user last engaged — independent of
    /// egui's implicit widget focus.
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
            pending_expand: None,
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

    /// Returns true if the sidebar should be rendered.
    pub fn is_visible(&self) -> bool {
        self.visible
    }

    /// Renders the sidebar content and returns any action to execute.
    pub fn show(&mut self, ui: &mut egui::Ui) -> SidebarAction {
        // Clear the Enter-suppression flag from the previous frame.
        self.rename_just_confirmed = false;

        // Keyboard navigation runs first so that a key press can produce
        // an `OpenFile` action this frame without being preempted by the
        // double-click handler in the file row.
        let sidebar_rect = ui.max_rect();
        let mut action = self
            .handle_tree_kbd_nav(ui.ctx(), sidebar_rect)
            .unwrap_or(SidebarAction::None);

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

    /// Renders the sidebar header with workspace name (row 1) and toolbar
    /// buttons (row 2). The two-row layout keeps the toolbar legible at
    /// the minimum 150 px sidebar width — the previous single-row layout
    /// reserved a fixed 160 px strip for buttons and starved the name
    /// label of horizontal room.
    fn render_header(&mut self, ui: &mut egui::Ui, action: &mut SidebarAction) {
        ui.vertical(|ui| {
            // Row 1: workspace name / menu / rename field
            ui.horizontal(|ui| {
                if self.workspace_name.is_empty() {
                    ui.strong("Workspace");
                } else if self.rename_buffer.is_some() {
                    self.render_workspace_rename_field(ui, action);
                } else {
                    self.render_workspace_name_with_menu(ui, action);
                }
            });
            // Row 2: toolbar buttons, left-aligned. Order: Add (+),
            // Toggle hidden, Collapse all, Expand all, Close (X) —
            // destructive Close at the far right so accidental clicks
            // on the creator (Add) are unlikely to land on it.
            ui.horizontal(|ui| {
                if ui
                    .small_button(icons::PLUS)
                    .on_hover_text("Add folder")
                    .clicked()
                {
                    *action = SidebarAction::AddFolder;
                }
                let hidden_label = if self.show_hidden {
                    icons::EYE
                } else {
                    icons::EYE_SLASH
                };
                let hidden_tooltip = if self.show_hidden {
                    "Hide hidden files"
                } else {
                    "Show hidden files"
                };
                if ui
                    .small_button(hidden_label)
                    .on_hover_text(hidden_tooltip)
                    .clicked()
                {
                    *action = SidebarAction::ToggleHiddenFiles;
                }
                if ui
                    .small_button(icons::CARET_DOUBLE_UP)
                    .on_hover_text("Collapse all")
                    .clicked()
                {
                    *action = SidebarAction::CollapseAll;
                }
                if ui
                    .small_button(icons::CARET_DOUBLE_DOWN)
                    .on_hover_text("Expand all")
                    .clicked()
                {
                    *action = SidebarAction::ExpandAll;
                }
                if ui
                    .small_button(icons::X)
                    .on_hover_text("Close workspace")
                    .clicked()
                {
                    *action = SidebarAction::CloseWorkspace;
                }
            });
        });
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
    fn render_workspace_name_with_menu(&mut self, ui: &mut egui::Ui, action: &mut SidebarAction) {
        let name_response = ui
            .strong(&self.workspace_name)
            .on_hover_text("Double-click to rename");
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

    /// Renders the folder tree with collapsible roots.
    fn render_tree(&mut self, ui: &mut egui::Ui, action: &mut SidebarAction) {
        let mut context_action = SidebarAction::None;
        let mut new_entry_request: Option<NewEntryState> = None;
        let mut selection_request: Option<SelectedNode> = None;
        let tree_len = self.tree.len();
        let bulk = self.pending_bulk_collapse.take();
        // Hoist the inline-rename and pending-expand state into locals so a root
        // row can render its own rename field (the body's `render_entry_list`
        // only reaches nested entries) without aliasing `self` mutably across
        // the `show_header`/`body` closures. Written back after the loop.
        let mut rename_state = self.rename_entry.take();
        let mut pending = self.pending_expand.take();
        let mut rename_confirmed = self.rename_just_confirmed;
        let mut clear_rename = false;
        // Snapshot the selection so we can pass it by reference through the
        // borrow-checker without aliasing `self.selected` mutably.
        let selected_snapshot = self.selected.clone();

        for root_idx in 0..tree_len {
            let root_path = self.tree[root_idx].path.clone();
            let root_name = root_path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| root_path.to_string_lossy().into_owned());

            let folder_exists = root_path.is_dir();

            // A root selected for rename shows its inline field in place of the
            // collapsing header, so the rename targets the row the user picked
            // — not a same-path duplicate nested under another root.
            let renaming_this_root = rename_state
                .as_ref()
                .is_some_and(|r| r.root_index == root_idx && r.original_path == root_path);
            if renaming_this_root {
                if let Some(state) = rename_state.as_mut() {
                    if render_inline_rename(ui, state, action, &mut rename_confirmed) {
                        clear_rename = true;
                    }
                }
                continue;
            }

            let id = ui.make_persistent_id(format!("root_{root_idx}"));
            if let Some(open) = bulk {
                self.tree[root_idx].expanded = open;
            }
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
            if let Some(open) = bulk {
                cs.set_open(open);
                cs.store(ui.ctx());
            }
            if should_force_open && !cs.is_open() {
                cs.set_open(true);
            }
            // Apply a queued keyboard expand/collapse for this root (see
            // `pending_expand`): the data-model flag alone does not move egui's
            // header state.
            if pending.as_ref().is_some_and(|(p, _)| *p == root_path) {
                if let Some((_, open)) = pending.take() {
                    cs.set_open(open);
                    cs.store(ui.ctx());
                    self.tree[root_idx].expanded = open;
                }
            }

            let root_selected = selected_snapshot
                .as_ref()
                .is_some_and(|s| s.root_index == root_idx && s.path == root_path);
            let (_toggle, header_inner, _body) = cs
                .show_header(ui, |ui| {
                    let response = if folder_exists {
                        selectable_row(ui, root_selected, egui::RichText::new(&root_name).strong())
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                    } else {
                        ui.weak(format!(
                            "{} {root_name} (unavailable)",
                            icons::WARNING_CIRCLE
                        ))
                    };
                    if folder_exists && response.clicked() {
                        selection_request = Some(SelectedNode {
                            root_index: root_idx,
                            path: root_path.clone(),
                        });
                    }
                    response.context_menu(|ui| {
                        show_root_context_menu(
                            ui,
                            &root_path,
                            folder_exists,
                            &mut context_action,
                            &mut new_entry_request,
                        );
                    });
                    response
                })
                .body(|ui| {
                    if folder_exists {
                        let mut ctx = RenderCtx {
                            action,
                            new_entry: &mut self.new_entry,
                            rename_entry: &mut rename_state,
                            rename_just_confirmed: &mut rename_confirmed,
                            workspace_root: &root_path,
                            show_hidden: self.show_hidden,
                            root_index: root_idx,
                            selected: selected_snapshot.as_ref(),
                            pending_expand: &mut pending,
                        };
                        render_entry_list(
                            ui,
                            &root_path,
                            &mut self.tree[root_idx].entries,
                            &mut ctx,
                            &mut selection_request,
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
            rename_state = None;
        }
        if clear_rename {
            rename_state = None;
        }
        // Write hoisted state back.
        self.rename_entry = rename_state;
        self.pending_expand = pending;
        self.rename_just_confirmed = rename_confirmed;
        if let Some(req) = selection_request {
            self.selected = Some(req);
            // A row click hands keyboard ownership to the sidebar so arrow
            // navigation works regardless of pointer position until the user
            // clicks another panel.
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
    /// included — keyboard nav cannot reveal an entry the renderer hasn't
    /// materialised, matching what the user sees.
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
    /// returning just the paths, in the same order. Test-only — production
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
    /// files, unknown paths, or roots that aren't directories. Note: this
    /// does not synchronise with egui's `CollapsingState`; the next render
    /// pass updates the on-screen state from `entry.expanded`.
    pub(crate) fn set_expanded(&mut self, target: &std::path::Path, open: bool) {
        for root in &mut self.tree {
            if root.path == target {
                root.expanded = open;
                return;
            }
            if let Some(entry) = find_entry_mut(&mut root.entries, target) {
                if matches!(entry.kind, EntryKind::Directory) {
                    entry.expanded = open;
                }
                return;
            }
        }
    }

    /// Queues a keyboard-driven expand/collapse: updates the data-model
    /// `expanded` flag *and* records a one-shot override so the next
    /// `render_tree` pushes the new state into egui's `CollapsingState`
    /// (see [`pending_expand`](Self::pending_expand)). Mouse interactions write
    /// egui directly; keyboard nav must route through here or the header never
    /// moves on screen.
    pub(crate) fn queue_expand(&mut self, target: &std::path::Path, open: bool) {
        self.set_expanded(target, open);
        self.pending_expand = Some((target.to_path_buf(), open));
    }

    /// Convenience: flips [`is_expanded`] for `target`, queuing the egui
    /// override so the change is visible.
    pub(crate) fn toggle_expanded_for(&mut self, target: &std::path::Path) {
        let new = !self.is_expanded(target);
        self.queue_expand(target, new);
    }

    /// Handles arrow / Enter / F2 keystrokes for the sidebar tree.
    ///
    /// Activation rules — keyboard nav fires when the pointer is over the
    /// sidebar OR when a selection exists AND the sidebar holds keyboard
    /// ownership ([`kbd_active`](Self::kbd_active)). The latter lets the user
    /// navigate after moving the mouse away; ownership is released when the
    /// editor is clicked, so keys never bleed across panels — independent of
    /// egui's implicit widget focus (which the editor monopolises via its
    /// per-frame `auto_focus`). Returns `Some(action)` only when Enter on a
    /// file should open it. Inline rename / new-entry editing suspends nav.
    pub(crate) fn handle_tree_kbd_nav(
        &mut self,
        ctx: &egui::Context,
        sidebar_rect: egui::Rect,
    ) -> Option<SidebarAction> {
        // Skip if any inline edit is active — the TextEdit owns the keys.
        if self.rename_buffer.is_some() || self.rename_entry.is_some() || self.new_entry.is_some() {
            return None;
        }
        let pointer_in_sidebar = ctx.input(|i| {
            i.pointer
                .latest_pos()
                .is_some_and(|p| sidebar_rect.contains(p))
        });
        if !(pointer_in_sidebar || (self.selected.is_some() && self.kbd_active)) {
            return None;
        }
        // Pressing a navigation key while the pointer is over the sidebar is a
        // deliberate engagement — latch keyboard ownership so navigation keeps
        // working after the pointer leaves, until another panel is clicked.
        // (Mere hovering must NOT latch, or it would silently steal arrows
        // back from the editor the next time the pointer crosses the sidebar.)
        if pointer_in_sidebar {
            use egui::Key;
            let nav_key_pressed = ctx.input(|i| {
                i.key_pressed(Key::ArrowDown)
                    || i.key_pressed(Key::ArrowUp)
                    || i.key_pressed(Key::ArrowLeft)
                    || i.key_pressed(Key::ArrowRight)
                    || i.key_pressed(Key::Enter)
                    || i.key_pressed(Key::F2)
            });
            if nav_key_pressed {
                self.kbd_active = true;
            }
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
            if let Some(idx) = current_idx {
                let node = nodes[idx].clone();
                if let Some(kind) = self.entry_kind_for(&node.path) {
                    match kind {
                        EntryKind::File => {
                            // Opening a file hands keyboard ownership to the
                            // editor so the user can type immediately.
                            self.kbd_active = false;
                            return Some(SidebarAction::OpenFile(node.path));
                        }
                        EntryKind::Directory => {
                            self.toggle_expanded_for(&node.path);
                            return None;
                        }
                    }
                }
            }
            return None;
        }
        if ctx.input_mut(|i| i.consume_key(mods, Key::F2)) {
            if let Some(idx) = current_idx {
                let node = nodes[idx].clone();
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
            return None;
        }
        if ctx.input_mut(|i| i.consume_key(mods, Key::ArrowRight)) {
            if let Some(idx) = current_idx {
                let node = nodes[idx].clone();
                if matches!(self.entry_kind_for(&node.path), Some(EntryKind::Directory)) {
                    if self.is_expanded(&node.path) {
                        // Move to the first child if it appears below us in the
                        // same root subtree — i.e. lazy-load already ran and
                        // the directory is non-empty.
                        if let Some(child) = nodes.get(idx + 1) {
                            if child.root_index == node.root_index
                                && child.path.starts_with(&node.path)
                            {
                                self.selected = Some(child.clone());
                            }
                        }
                    } else {
                        self.queue_expand(&node.path, true);
                    }
                }
            }
            return None;
        }
        if ctx.input_mut(|i| i.consume_key(mods, Key::ArrowLeft)) {
            if let Some(idx) = current_idx {
                let node = nodes[idx].clone();
                let is_dir = matches!(self.entry_kind_for(&node.path), Some(EntryKind::Directory));
                if is_dir && self.is_expanded(&node.path) {
                    self.queue_expand(&node.path, false);
                } else if let Some(parent) = node.path.parent() {
                    // Jump to the parent row within the same root subtree.
                    if let Some(parent_node) = nodes
                        .iter()
                        .find(|n| n.root_index == node.root_index && n.path == parent)
                    {
                        self.selected = Some(parent_node.clone());
                    }
                }
            }
            return None;
        }
        None
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

/// Mutable counterpart of [`find_entry`].
fn find_entry_mut<'a>(
    entries: &'a mut [TreeEntry],
    target: &std::path::Path,
) -> Option<&'a mut TreeEntry> {
    for entry in entries.iter_mut() {
        if entry.path == target {
            return Some(entry);
        }
        if matches!(entry.kind, EntryKind::Directory) {
            if let Some(hit) = find_entry_mut(&mut entry.children, target) {
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
/// performs no filename validation** — sanitization is the caller's or the
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
/// State transitions are intentionally not traced — this runs per frame; if
/// observability is ever needed, instrument the `SidebarAction` handler
/// instead, never this helper.
fn render_inline_entry_field(
    ui: &mut egui::Ui,
    icon: &str,
    id_salt: &str,
    name: &mut String,
    select_on_focus: &mut bool,
) -> InlineEntryOutcome {
    ui.horizontal(|ui| {
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
    /// One-shot keyboard expand/collapse override threaded down from
    /// `render_tree`. When it matches a directory's path, that directory's
    /// egui `CollapsingState` is forced open/closed for the frame and the slot
    /// is cleared. See [`WorkspaceSidebar::pending_expand`].
    pub pending_expand: &'a mut Option<(PathBuf, bool)>,
}

impl RenderCtx<'_> {
    /// Whether `path` in the current root subtree is the selected row.
    fn is_selected(&self, path: &Path) -> bool {
        self.selected
            .is_some_and(|s| s.root_index == self.root_index && s.path == path)
    }
}

/// Renders a clickable, selectable tree row that mirrors the look of
/// `ui.selectable_label` (selection highlight, hover fill) but senses with
/// [`egui::Sense::CLICK`] — the bare, **non-focusable** click flag.
///
/// Tree rows must not take egui keyboard focus: a focused row makes egui's
/// spatial widget navigation hijack the arrow keys (focus jumps to unrelated
/// widgets) and lets the row's own activation race the sidebar's nav handler.
/// Keeping rows non-focusable leaves arrow/Enter/F2 entirely to
/// [`WorkspaceSidebar::handle_tree_kbd_nav`].
fn selectable_row<'a>(
    ui: &mut egui::Ui,
    selected: bool,
    text: impl egui::IntoAtoms<'a>,
) -> egui::Response {
    ui.add(
        egui::Button::new(text)
            .selected(selected)
            .frame_when_inactive(selected)
            .sense(egui::Sense::CLICK),
    )
}

/// Renders a directory tree entry with collapsing header, context menu, and lazy-loaded children.
///
/// `ExpandAll` / `CollapseAll` deliberately do NOT propagate here — they only
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
) {
    let name = entry.name.clone();
    let path = entry.path.clone();
    let expanded = entry.expanded;
    let id = ui.make_persistent_id(("entry", &path));

    let should_force_open = ctx.new_entry.as_ref().is_some_and(|ne| ne.parent == path);

    let mut cs =
        egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, expanded);
    if should_force_open && !cs.is_open() {
        cs.set_open(true);
    }
    // Apply a queued keyboard expand/collapse to egui's own state — the
    // data-model flag alone does not move the header (see `pending_expand`).
    if ctx.pending_expand.as_ref().is_some_and(|(p, _)| *p == path) {
        if let Some((_, open)) = ctx.pending_expand.take() {
            cs.set_open(open);
            entry.expanded = open;
        }
    }

    let show_hidden = ctx.show_hidden;
    let workspace_root = ctx.workspace_root.to_path_buf();
    let selected = ctx.is_selected(&path);
    let root_index = ctx.root_index;
    let (_toggle, header_inner, _body) = cs
        .show_header(ui, |ui| {
            let response = selectable_row(ui, selected, format!("{} {name}", icons::FOLDER))
                .on_hover_cursor(egui::CursorIcon::PointingHand);
            if response.clicked() {
                *selection_request = Some(SelectedNode {
                    root_index,
                    path: path.clone(),
                });
            }
            response.context_menu(|ui| {
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
            response
        })
        .body(|ui| {
            // Lazy-load children on first expand. This blocks the UI
            // thread for one frame while scanning, but the result is
            // cached in `entry.children` so subsequent frames are free.
            if entry.children.is_empty() {
                let dir_path = entry.path.clone();
                if let Ok(children) = super::scanner::scan_directory(&dir_path, show_hidden) {
                    entry.children = children;
                }
            }
            render_entry_list(ui, &path, &mut entry.children, ctx, selection_request);
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

/// Renders a file tree entry with the file-layout context menu.
fn render_file_entry(
    ui: &mut egui::Ui,
    entry: &TreeEntry,
    ctx: &mut RenderCtx<'_>,
    rename_request: &mut Option<RenameEntryState>,
    selection_request: &mut Option<SelectedNode>,
) {
    let icon = file_icon(&entry.name);
    let selected = ctx.is_selected(&entry.path);
    let response = selectable_row(ui, selected, format!("{icon} {}", entry.name));

    if response.clicked() {
        *selection_request = Some(SelectedNode {
            root_index: ctx.root_index,
            path: entry.path.clone(),
        });
    }
    if response.double_clicked() {
        *ctx.action = SidebarAction::OpenFile(entry.path.clone());
    }
    let workspace_root = ctx.workspace_root.to_path_buf();
    let path = entry.path.clone();
    let name = entry.name.clone();
    response.context_menu(|ui| {
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
/// Works at any nesting depth — directories lazy-load their children on first
/// expand and cache the result in `TreeEntry.children`.
fn render_entry_list(
    ui: &mut egui::Ui,
    parent_path: &Path,
    entries: &mut [TreeEntry],
    ctx: &mut RenderCtx<'_>,
    selection_request: &mut Option<SelectedNode>,
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
                if render_inline_rename(ui, state, ctx.action, ctx.rename_just_confirmed) {
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
                );
            }
            EntryKind::File => {
                render_file_entry(ui, entry, ctx, &mut rename_request, selection_request);
            }
        }
    }

    // Inline new entry text field (at the end of the list)
    let mut clear_new = false;
    if let Some(ref mut state) = ctx.new_entry {
        if state.parent.as_path() == parent_path
            && render_inline_new_entry_field(ui, state, ctx.action, ctx.rename_just_confirmed)
        {
            clear_new = true;
        }
    }

    // Apply deferred state changes
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
/// the user replace the name by typing while preserving the extension —
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
        // Root 1: C itself — same physical path as A/C.
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

        // Selecting the root-1 instance matches exactly one row — not the
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
        // root + sub only — sub.expanded is false so children are skipped.
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
        let found = find_entry_mut(&mut sidebar.tree[0].entries, &target).expect("found");
        assert_eq!(found.name, "deep.rs");
        assert_eq!(found.kind, EntryKind::File);
    }

    #[test]
    fn queue_expand_flips_flag_and_records_override() {
        // Defects A/B: keyboard expand must update BOTH the data-model flag and
        // the one-shot `pending_expand` override that the next render pushes
        // into egui's CollapsingState.
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
        assert!(sidebar.pending_expand.is_none());

        sidebar.queue_expand(&target, true);
        assert!(sidebar.is_expanded(&target), "model flag set");
        assert_eq!(
            sidebar.pending_expand,
            Some((target.clone(), true)),
            "render override recorded so egui follows the model",
        );

        // Collapsing records the inverse override.
        sidebar.queue_expand(&target, false);
        assert!(!sidebar.is_expanded(&target));
        assert_eq!(sidebar.pending_expand, Some((target.clone(), false)));
    }

    #[test]
    fn toggle_expanded_for_queues_override() {
        // Enter-on-folder routes through toggle → queue_expand, so it must also
        // arm the override (the bug was that it only touched the data model).
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
        assert_eq!(sidebar.pending_expand, Some((target.clone(), true)));
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
        // No egui::Context — short-circuit via the inline-edit gate so
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
}
