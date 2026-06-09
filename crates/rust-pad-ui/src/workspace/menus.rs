//! Context-menu builders for the workspace sidebar.
//!
//! The three top-level builders (`show_file_context_menu`,
//! `show_directory_context_menu`, `show_root_context_menu`) produce the
//! per-entry menu layouts. They mutate
//! [`RenderCtx::action`] and the outgoing `*_request` slots used by
//! `render_entry_list`. Keeping them out of `sidebar.rs` keeps that file
//! focused on rendering geometry; this module owns the menu *vocabulary*.

use std::path::{Path, PathBuf};

use eframe::egui;

use super::sidebar::{CopyPathScope, NewEntryState, RenameEntryState, RenderCtx, SidebarAction};
use crate::app::workspace_ops::generate_unique_name;

/// A request to copy a representation of `path` to the clipboard.
///
/// Neutral carrier so every call site (workspace tree, single-pane tab bar,
/// per-pane tab bar) can share one submenu builder ([`copy_path_menu`]) without
/// the builder being tied to any one caller's action enum. Each caller maps the
/// emitted request into its own deferred-action type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CopyPathRequest {
    /// The entry whose path is being copied.
    pub path: PathBuf,
    /// Root used to compute the relative scope. For the `Name`/`Full` scopes
    /// this is irrelevant (the handler ignores it) and is set to `path`.
    pub root: PathBuf,
    /// Which representation to copy.
    pub scope: CopyPathScope,
}

/// Renders the `Copy Path > {Name | Full Path | Relative Path}` submenu and
/// writes the chosen request into `out`.
///
/// `relative_root` is `Some(root)` when a relative representation is meaningful
/// (the entry lives under a known root); the `Relative Path` item is then
/// enabled and the request carries that root. When `relative_root` is `None`
/// the `Relative Path` item is rendered **disabled** — `Name` and `Full Path`
/// remain available. `Name`/`Full Path` set the request root to `path` since
/// those scopes do not consult it.
pub(crate) fn copy_path_menu(
    ui: &mut egui::Ui,
    path: &Path,
    relative_root: Option<&Path>,
    out: &mut Option<CopyPathRequest>,
) {
    ui.menu_button("Copy Path", |ui| {
        copy_path_menu_items(ui, path, relative_root, out);
    });
}

/// Builds the request for a chosen `scope`.
///
/// `Name`/`Full` ignore the root (set to `path`). `Relative` needs the
/// containing root; returns `None` when it is absent — though the caller
/// disables that item in that case, so the `None` branch is a safety net.
fn copy_path_request(
    path: &Path,
    relative_root: Option<&Path>,
    scope: CopyPathScope,
) -> Option<CopyPathRequest> {
    let root = match scope {
        CopyPathScope::Relative => relative_root?,
        CopyPathScope::Name | CopyPathScope::Full => path,
    };
    Some(CopyPathRequest {
        path: path.to_path_buf(),
        root: root.to_path_buf(),
        scope,
    })
}

/// Renders the three submenu items. Split out from [`copy_path_menu`] so the
/// item bodies are reachable without opening an egui popup (the `menu_button`
/// closure only runs while the menu is open), which keeps them testable.
fn copy_path_menu_items(
    ui: &mut egui::Ui,
    path: &Path,
    relative_root: Option<&Path>,
    out: &mut Option<CopyPathRequest>,
) {
    if ui.button("Name").clicked() {
        *out = copy_path_request(path, relative_root, CopyPathScope::Name);
        ui.close();
    }
    if ui.button("Full Path").clicked() {
        *out = copy_path_request(path, relative_root, CopyPathScope::Full);
        ui.close();
    }
    let relative = ui.add_enabled(relative_root.is_some(), egui::Button::new("Relative Path"));
    if relative.clicked() {
        *out = copy_path_request(path, relative_root, CopyPathScope::Relative);
        ui.close();
    }
}

/// Renders the "New File..." / "New Folder..." pair used at the top of the
/// folder/root menus. The chosen entry is written into `new_entry_request`,
/// which `render_entry_list` consumes after the loop returns.
pub(crate) fn show_new_entry_menu(
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
            select_on_focus: true,
        });
        ui.close();
    }
    if ui.button("New Folder...").clicked() {
        let name = generate_unique_name(parent, "new_folder", true);
        *new_entry_request = Some(NewEntryState {
            parent: parent.to_path_buf(),
            name,
            is_dir: true,
            select_on_focus: true,
        });
        ui.close();
    }
}

/// Renders the `Copy Path` submenu containing Name / Full Path / Relative
/// Path for a workspace-tree entry. Thin adapter over [`copy_path_menu`] that
/// maps the emitted request into `ctx_action`. Tree entries always have a
/// containing root, so the relative scope is always enabled here.
pub(crate) fn show_copy_path_submenu(
    ui: &mut egui::Ui,
    path: &Path,
    workspace_root: &Path,
    ctx_action: &mut SidebarAction,
) {
    let mut request = None;
    copy_path_menu(ui, path, Some(workspace_root), &mut request);
    if let Some(req) = request {
        *ctx_action = SidebarAction::CopyPath {
            path: req.path,
            root: req.root,
            scope: req.scope,
        };
    }
}

/// Builds the context menu shown when right-clicking a file entry in the
/// tree.
pub(crate) fn show_file_context_menu(
    ui: &mut egui::Ui,
    file_path: &Path,
    file_name: &str,
    workspace_root: &Path,
    ctx: &mut RenderCtx<'_>,
    rename_request: &mut Option<RenameEntryState>,
) {
    if ui.button("Open").clicked() {
        *ctx.action = SidebarAction::OpenFile(file_path.to_path_buf());
        ui.close();
    }
    if ui.button("Copy Contents").clicked() {
        *ctx.action = SidebarAction::CopyFileContents {
            path: file_path.to_path_buf(),
            workspace_root: workspace_root.to_path_buf(),
        };
        ui.close();
    }
    ui.separator();
    show_copy_path_submenu(ui, file_path, workspace_root, ctx.action);
    ui.separator();
    if ui.button("Rename").clicked() {
        *rename_request = Some(RenameEntryState {
            original_path: file_path.to_path_buf(),
            root_index: ctx.root_index,
            name: file_name.to_string(),
            is_dir: false,
            select_on_focus: true,
        });
        ui.close();
    }
    if ui.button("Delete").clicked() {
        *ctx.action = SidebarAction::DeleteFile(file_path.to_path_buf());
        ui.close();
    }
}

/// Builds the context menu shown when right-clicking a directory entry in
/// the tree.
pub(crate) fn show_directory_context_menu(
    ui: &mut egui::Ui,
    dir_path: &Path,
    dir_name: &str,
    workspace_root: &Path,
    ctx: &mut RenderCtx<'_>,
    new_entry_request: &mut Option<NewEntryState>,
    rename_request: &mut Option<RenameEntryState>,
) {
    if ui.button("Open in File Explorer").clicked() {
        *ctx.action = SidebarAction::OpenInFileExplorer(dir_path.to_path_buf());
        ui.close();
    }
    if ui.button("Reload from disk").clicked() {
        *ctx.action = SidebarAction::ReloadFromDisk(dir_path.to_path_buf());
        ui.close();
    }
    ui.separator();
    show_new_entry_menu(ui, dir_path, new_entry_request);
    ui.separator();
    show_copy_path_submenu(ui, dir_path, workspace_root, ctx.action);
    ui.separator();
    if ui.button("Rename").clicked() {
        *rename_request = Some(RenameEntryState {
            original_path: dir_path.to_path_buf(),
            root_index: ctx.root_index,
            name: dir_name.to_string(),
            is_dir: true,
            select_on_focus: true,
        });
        ui.close();
    }
    if ui.button("Delete").clicked() {
        *ctx.action = SidebarAction::DeleteFile(dir_path.to_path_buf());
        ui.close();
    }
}

/// Builds the context menu shown when right-clicking a workspace-root
/// header. The root is its own `workspace_root`,
/// so the relative-path scope degenerates to the folder name.
///
/// Returns through the two output parameters because the existing
/// `render_tree` already uses `context_action` and `new_entry_request`
/// captures to side-step the egui closure's borrow constraints.
pub(crate) fn show_root_context_menu(
    ui: &mut egui::Ui,
    root_path: &Path,
    folder_exists: bool,
    context_action: &mut SidebarAction,
    new_entry_request: &mut Option<NewEntryState>,
) {
    if folder_exists {
        if ui.button("Open in File Explorer").clicked() {
            *context_action = SidebarAction::OpenInFileExplorer(root_path.to_path_buf());
            ui.close();
        }
        if ui.button("Reload from disk").clicked() {
            *context_action = SidebarAction::ReloadFromDisk(root_path.to_path_buf());
            ui.close();
        }
        ui.separator();
        show_new_entry_menu(ui, root_path, new_entry_request);
        ui.separator();
        show_copy_path_submenu(ui, root_path, root_path, context_action);
        ui.separator();
    }
    if ui.button("Remove from Workspace").clicked() {
        *context_action = SidebarAction::RemoveFolder(root_path.to_path_buf());
        ui.close();
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use eframe::egui;

    use super::*;

    // The menu builders themselves are egui-driven and need a UI to
    // exercise; the action-producing logic is covered by the
    // `handle_sidebar_action` arms in `app/workspace_ops.rs`. These
    // tests pin the shape of the action enums that the builders emit so
    // a future restructure does not silently change the contract.

    // ── copy_path_request (pure scope → request mapping) ──

    #[test]
    fn copy_path_request_name_uses_path_as_root() {
        let req = copy_path_request(
            Path::new("/p/a.rs"),
            Some(Path::new("/p")),
            CopyPathScope::Name,
        )
        .expect("Name always yields a request");
        assert_eq!(req.root, PathBuf::from("/p/a.rs"));
        assert_eq!(req.scope, CopyPathScope::Name);
    }

    #[test]
    fn copy_path_request_full_uses_path_as_root_even_without_relative_root() {
        let req = copy_path_request(Path::new("/p/a.rs"), None, CopyPathScope::Full)
            .expect("Full always yields a request");
        assert_eq!(req.root, PathBuf::from("/p/a.rs"));
        assert_eq!(req.scope, CopyPathScope::Full);
    }

    #[test]
    fn copy_path_request_relative_uses_relative_root() {
        let req = copy_path_request(
            Path::new("/p/a.rs"),
            Some(Path::new("/p")),
            CopyPathScope::Relative,
        )
        .expect("Relative with a root yields a request");
        assert_eq!(req.root, PathBuf::from("/p"));
        assert_eq!(req.scope, CopyPathScope::Relative);
    }

    #[test]
    fn copy_path_request_relative_without_root_is_none() {
        assert!(copy_path_request(Path::new("/p/a.rs"), None, CopyPathScope::Relative).is_none());
    }

    // ── copy_path_menu_items (headless egui drive) ──
    //
    // Items render directly into the top-level `run_ui` Ui with zero
    // padding/spacing and a forced 24px interact height, so the three
    // buttons occupy fixed vertical bands starting at the origin and clicks
    // land deterministically: band i centre is at y = 24*i + 12 → 12 / 36 /
    // 60. Any in-bounds x works; the narrowest button ("Name") is ~34px
    // wide, so x = 12 is safe.

    /// Centre y of menu-item band `i` (Name=0, Full=1, Relative=2).
    fn band_y(i: f32) -> f32 {
        24.0 * i + 12.0
    }

    fn screen_input(events: Vec<egui::Event>) -> egui::RawInput {
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::pos2(0.0, 0.0),
                egui::vec2(300.0, 300.0),
            )),
            events,
            ..Default::default()
        }
    }

    fn drive_items(
        relative_root: Option<&Path>,
        click: Option<egui::Pos2>,
    ) -> Option<CopyPathRequest> {
        let ctx = egui::Context::default();
        let path = Path::new("/proj/src/main.rs");
        let mut out = None;
        let frame = |events: Vec<egui::Event>, out: &mut Option<CopyPathRequest>| {
            let _ = ctx.run_ui(screen_input(events), |ui| {
                ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
                ui.spacing_mut().button_padding = egui::vec2(0.0, 0.0);
                ui.spacing_mut().interact_size = egui::vec2(120.0, 24.0);
                copy_path_menu_items(ui, path, relative_root, out);
            });
        };
        match click {
            None => frame(Vec::new(), &mut out),
            Some(p) => {
                // Warmup frame: lay out the buttons so egui knows their rects
                // (interaction hit-tests against the previous frame's geometry).
                frame(vec![egui::Event::PointerMoved(p)], &mut out);
                // Press over the target.
                frame(
                    vec![egui::Event::PointerButton {
                        pos: p,
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers: egui::Modifiers::NONE,
                    }],
                    &mut out,
                );
                // Release → the click fires on this frame.
                frame(
                    vec![egui::Event::PointerButton {
                        pos: p,
                        button: egui::PointerButton::Primary,
                        pressed: false,
                        modifiers: egui::Modifiers::NONE,
                    }],
                    &mut out,
                );
            }
        }
        out
    }

    #[test]
    fn copy_path_menu_items_render_without_click_emits_nothing() {
        assert!(drive_items(Some(Path::new("/proj")), None).is_none());
        assert!(drive_items(None, None).is_none());
    }

    #[test]
    fn copy_path_menu_items_name_click_emits_name() {
        let req = drive_items(
            Some(Path::new("/proj")),
            Some(egui::pos2(12.0, band_y(0.0))),
        )
        .expect("clicking Name emits a request");
        assert_eq!(req.scope, CopyPathScope::Name);
    }

    #[test]
    fn copy_path_menu_items_full_click_emits_full() {
        let req = drive_items(
            Some(Path::new("/proj")),
            Some(egui::pos2(12.0, band_y(1.0))),
        )
        .expect("clicking Full Path emits a request");
        assert_eq!(req.scope, CopyPathScope::Full);
    }

    #[test]
    fn copy_path_menu_items_relative_click_emits_relative_with_root() {
        let req = drive_items(
            Some(Path::new("/proj")),
            Some(egui::pos2(12.0, band_y(2.0))),
        )
        .expect("clicking Relative Path emits a request");
        assert_eq!(req.scope, CopyPathScope::Relative);
        assert_eq!(req.root, PathBuf::from("/proj"));
    }

    #[test]
    fn copy_path_menu_items_relative_disabled_click_is_noop() {
        // No containing root → the Relative item is disabled and a click on it
        // does nothing.
        assert!(drive_items(None, Some(egui::pos2(12.0, band_y(2.0)))).is_none());
    }

    #[test]
    fn copy_path_menu_and_submenu_render_without_panic() {
        let ctx = egui::Context::default();
        let _ = ctx.run_ui(screen_input(Vec::new()), |ui| {
            let mut out = None;
            copy_path_menu(ui, Path::new("/p/a.rs"), Some(Path::new("/p")), &mut out);
            let mut action = SidebarAction::None;
            show_copy_path_submenu(ui, Path::new("/p/a.rs"), Path::new("/p"), &mut action);
        });
    }

    #[test]
    fn copy_path_scopes_are_distinct() {
        assert_ne!(CopyPathScope::Name, CopyPathScope::Full);
        assert_ne!(CopyPathScope::Name, CopyPathScope::Relative);
        assert_ne!(CopyPathScope::Full, CopyPathScope::Relative);
    }

    #[test]
    fn copy_path_action_carries_path_root_and_scope() {
        let action = SidebarAction::CopyPath {
            path: PathBuf::from("/proj/src/main.rs"),
            root: PathBuf::from("/proj"),
            scope: CopyPathScope::Relative,
        };
        match action {
            SidebarAction::CopyPath { path, root, scope } => {
                assert_eq!(path, PathBuf::from("/proj/src/main.rs"));
                assert_eq!(root, PathBuf::from("/proj"));
                assert_eq!(scope, CopyPathScope::Relative);
            }
            _ => panic!("expected CopyPath"),
        }
    }

    #[test]
    fn copy_file_contents_action_pairs_path_and_workspace_root() {
        let action = SidebarAction::CopyFileContents {
            path: PathBuf::from("/proj/README.md"),
            workspace_root: PathBuf::from("/proj"),
        };
        match action {
            SidebarAction::CopyFileContents {
                path,
                workspace_root,
            } => {
                assert_eq!(path, PathBuf::from("/proj/README.md"));
                assert_eq!(workspace_root, PathBuf::from("/proj"));
            }
            _ => panic!("expected CopyFileContents"),
        }
    }

    #[test]
    fn copy_path_request_carries_path_root_and_scope() {
        let req = CopyPathRequest {
            path: PathBuf::from("/proj/src/main.rs"),
            root: PathBuf::from("/proj"),
            scope: CopyPathScope::Relative,
        };
        assert_eq!(req.path, PathBuf::from("/proj/src/main.rs"));
        assert_eq!(req.root, PathBuf::from("/proj"));
        assert_eq!(req.scope, CopyPathScope::Relative);
    }

    #[test]
    fn open_in_file_explorer_action_carries_path() {
        let action = SidebarAction::OpenInFileExplorer(PathBuf::from("/proj/src"));
        match action {
            SidebarAction::OpenInFileExplorer(p) => {
                assert_eq!(p, PathBuf::from("/proj/src"));
            }
            _ => panic!("expected OpenInFileExplorer"),
        }
    }

    #[test]
    fn reload_from_disk_action_carries_path() {
        let action = SidebarAction::ReloadFromDisk(PathBuf::from("/proj/src"));
        match action {
            SidebarAction::ReloadFromDisk(p) => {
                assert_eq!(p, PathBuf::from("/proj/src"));
            }
            _ => panic!("expected ReloadFromDisk"),
        }
    }
}
