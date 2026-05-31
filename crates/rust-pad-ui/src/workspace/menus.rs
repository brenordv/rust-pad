//! Context-menu builders for the workspace sidebar.
//!
//! The three top-level builders (`show_file_context_menu`,
//! `show_directory_context_menu`, `show_root_context_menu`) produce the
//! per-entry menu layouts described in plan §3.1. They mutate
//! [`RenderCtx::action`] and the outgoing `*_request` slots used by
//! `render_entry_list`. Keeping them out of `sidebar.rs` keeps that file
//! focused on rendering geometry; this module owns the menu *vocabulary*.

use std::path::Path;

use eframe::egui;

use super::sidebar::{CopyPathScope, NewEntryState, RenameEntryState, RenderCtx, SidebarAction};
use crate::app::workspace_ops::generate_unique_name;

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
/// Path. The submenu mutates `ctx.action` directly so the calling menu
/// closure does not have to plumb a return value.
pub(crate) fn show_copy_path_submenu(
    ui: &mut egui::Ui,
    path: &Path,
    workspace_root: &Path,
    ctx_action: &mut SidebarAction,
) {
    ui.menu_button("Copy Path", |ui| {
        if ui.button("Name").clicked() {
            *ctx_action = SidebarAction::CopyPath {
                path: path.to_path_buf(),
                root: workspace_root.to_path_buf(),
                scope: CopyPathScope::Name,
            };
            ui.close();
        }
        if ui.button("Full Path").clicked() {
            *ctx_action = SidebarAction::CopyPath {
                path: path.to_path_buf(),
                root: workspace_root.to_path_buf(),
                scope: CopyPathScope::Full,
            };
            ui.close();
        }
        if ui.button("Relative Path").clicked() {
            *ctx_action = SidebarAction::CopyPath {
                path: path.to_path_buf(),
                root: workspace_root.to_path_buf(),
                scope: CopyPathScope::Relative,
            };
            ui.close();
        }
    });
}

/// Builds the context menu shown when right-clicking a file entry in the
/// tree (plan §3.1, file layout).
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
/// the tree (plan §3.1, directory layout).
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
    ui.separator();
    show_new_entry_menu(ui, dir_path, new_entry_request);
    ui.separator();
    show_copy_path_submenu(ui, dir_path, workspace_root, ctx.action);
    ui.separator();
    if ui.button("Rename").clicked() {
        *rename_request = Some(RenameEntryState {
            original_path: dir_path.to_path_buf(),
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
/// header (plan §3.1, root layout). The root is its own `workspace_root`,
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

    use super::*;

    // The menu builders themselves are egui-driven and need a UI to
    // exercise; the action-producing logic is covered by the
    // `handle_sidebar_action` arms in `app/workspace_ops.rs`. These
    // tests pin the shape of the action enums that the builders emit so
    // a future restructure does not silently change the contract.

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
    fn open_in_file_explorer_action_carries_path() {
        let action = SidebarAction::OpenInFileExplorer(PathBuf::from("/proj/src"));
        match action {
            SidebarAction::OpenInFileExplorer(p) => {
                assert_eq!(p, PathBuf::from("/proj/src"));
            }
            _ => panic!("expected OpenInFileExplorer"),
        }
    }
}
