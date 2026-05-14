//! Drag-and-drop handling for files and folders.
//!
//! Listens for OS file-drop events surfaced by egui and routes each
//! dropped path accordingly: files are opened in a new tab via
//! [`App::open_file_path`], and directories are added to the active
//! workspace via [`App::add_folder_path_to_workspace`].
//! Also renders a translucent overlay while items are being hovered
//! over the window.

use std::path::Path;

use eframe::egui;

use super::App;

impl App {
    /// Checks for items dropped onto the window and routes them.
    ///
    /// Files are opened in new tabs; directories are added to the
    /// active workspace (auto-creating one if needed).
    /// Should be called once per frame, early in the `ui()` method.
    /// Drops are ignored while a modal dialog is open.
    pub(crate) fn handle_dropped_items(&mut self, ctx: &egui::Context) {
        // Ignore drops while a dialog is showing to avoid confusing state
        if self.is_dialog_open() {
            return;
        }

        let dropped: Vec<_> = ctx.input(|i| i.raw.dropped_files.clone());

        let mut folders_to_add = Vec::new();

        for file in &dropped {
            if let Some(path) = &file.path {
                if path.is_dir() {
                    folders_to_add.push(path.clone());
                } else {
                    self.open_file_path(path);
                }
            }
        }

        for folder in &folders_to_add {
            self.add_dropped_folder(folder);
        }
    }

    /// Adds a dropped folder to the active workspace.
    ///
    /// If no workspace is currently active, creates one first.
    /// Also ensures the sidebar is visible so the user can see the result.
    fn add_dropped_folder(&mut self, folder_path: &Path) {
        if self.workspace_sidebar.workspace_id.is_none() {
            self.create_new_workspace();
        }
        self.workspace_sidebar.visible = true;
        self.add_folder_path_to_workspace(folder_path);
    }

    /// Paints a translucent overlay when the user hovers items over the window.
    pub(crate) fn paint_drop_overlay(&self, ctx: &egui::Context) {
        let hovering = ctx.input(|i| !i.raw.hovered_files.is_empty());
        if !hovering {
            return;
        }

        let screen = ctx.input(|i| i.content_rect());
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new("drop_overlay"),
        ));

        // Semi-transparent background
        painter.rect_filled(screen, 0.0, egui::Color32::from_black_alpha(160));

        // Centered label
        let text = egui::RichText::new("Drop file(s) or folder(s)")
            .size(24.0)
            .color(egui::Color32::WHITE);
        painter.text(
            screen.center(),
            egui::Align2::CENTER_CENTER,
            text.text(),
            egui::FontId::proportional(24.0),
            egui::Color32::WHITE,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::app_with_workspace;

    #[test]
    fn test_add_dropped_folder_with_active_workspace() {
        let (mut app, _dir) = app_with_workspace();
        let folder = tempfile::tempdir().unwrap();
        std::fs::write(folder.path().join("hello.rs"), "fn main() {}").unwrap();

        app.create_workspace("Drop Test");
        app.add_dropped_folder(folder.path());

        assert_eq!(app.workspace_sidebar.tree.len(), 1);
        assert_eq!(app.workspace_sidebar.tree[0].path, folder.path());
        assert!(!app.workspace_sidebar.tree[0].entries.is_empty());
    }

    #[test]
    fn test_add_dropped_folder_without_active_workspace() {
        let (mut app, _dir) = app_with_workspace();
        let folder = tempfile::tempdir().unwrap();
        std::fs::write(folder.path().join("test.txt"), "content").unwrap();

        // No workspace active — add_dropped_folder should auto-create one
        assert!(app.workspace_sidebar.workspace_id.is_none());
        app.add_dropped_folder(folder.path());

        assert!(app.workspace_sidebar.workspace_id.is_some());
        assert!(app.workspace_sidebar.visible);
        assert_eq!(app.workspace_sidebar.workspace_name, "New Workspace");
        assert_eq!(app.workspace_sidebar.tree.len(), 1);
        assert_eq!(app.workspace_sidebar.tree[0].path, folder.path());
    }

    #[test]
    fn test_add_dropped_folder_duplicate_rejected() {
        let (mut app, _dir) = app_with_workspace();
        let folder = tempfile::tempdir().unwrap();

        app.create_workspace("Dup Drop");
        app.add_dropped_folder(folder.path());
        assert_eq!(app.workspace_sidebar.tree.len(), 1);

        // Dropping the same folder again should be rejected
        app.add_dropped_folder(folder.path());
        assert_eq!(app.workspace_sidebar.tree.len(), 1);
    }

    #[test]
    fn test_add_dropped_folder_makes_sidebar_visible() {
        let (mut app, _dir) = app_with_workspace();
        let folder = tempfile::tempdir().unwrap();

        app.create_workspace("Visibility Test");
        app.workspace_sidebar.visible = false;

        app.add_dropped_folder(folder.path());
        assert!(app.workspace_sidebar.visible);
    }
}
