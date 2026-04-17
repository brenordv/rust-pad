//! Drag-and-drop file handling.
//!
//! Listens for OS file-drop events surfaced by egui and opens each
//! dropped file in a new tab via the existing [`App::open_file_path`]
//! pipeline. Also renders a translucent overlay while files are being
//! hovered over the window.

use eframe::egui;

use super::App;

impl App {
    /// Checks for files dropped onto the window and opens them.
    ///
    /// Should be called once per frame, early in the `ui()` method.
    /// Drops are ignored while a modal dialog is open.
    pub(crate) fn handle_dropped_files(&mut self, ctx: &egui::Context) {
        // Ignore drops while a dialog is showing to avoid confusing state
        if self.is_dialog_open() {
            return;
        }

        let dropped: Vec<_> = ctx.input(|i| i.raw.dropped_files.clone());

        for file in &dropped {
            if let Some(path) = &file.path {
                self.open_file_path(path);
            }
        }
    }

    /// Paints a translucent overlay when the user hovers files over the window.
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
        let text = egui::RichText::new("Drop file(s) to open")
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
