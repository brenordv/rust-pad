//! Right-click context menu for the editor area.
//!
//! Provides standard clipboard actions (Cut, Copy, Paste, Delete),
//! selection actions (Select All, Invert Selection), and scoped
//! text operations (Convert Case, Line Operations) that can apply
//! to either the entire document or the current selection.

use eframe::egui;
use rust_pad_core::line_ops::{CaseConversion, SortOrder};

use super::App;

/// Determines the scope of a text operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OperationScope {
    /// Operate on the entire document.
    Global,
    /// Operate only on the selected text/lines.
    Selection,
}

impl App {
    /// Renders the editor area context menu (right-click menu).
    pub(crate) fn show_editor_context_menu(&mut self, ui: &mut egui::Ui) {
        let has_selection = self.tabs.active_doc().selected_text().is_some();

        // Cut / Copy / Paste / Delete
        if ui
            .add_enabled(has_selection, egui::Button::new("Cut"))
            .clicked()
        {
            self.cut();
            ui.close();
        }
        if ui
            .add_enabled(has_selection, egui::Button::new("Copy"))
            .clicked()
        {
            self.copy();
            ui.close();
        }
        if ui.button("Paste").clicked() {
            self.paste();
            ui.close();
        }
        if ui
            .add_enabled(has_selection, egui::Button::new("Delete"))
            .clicked()
        {
            self.delete_selection_or_char();
            ui.close();
        }
        ui.separator();

        // Select All / Invert Selection
        if ui.button("Select All").clicked() {
            let doc = self.tabs.active_doc_mut();
            doc.cursor.select_all(&doc.buffer);
            ui.close();
        }
        if ui.button("Invert Selection").clicked() {
            self.invert_selection();
            ui.close();
        }
        ui.separator();

        // Global Operations (always available)
        self.show_operations_submenu(ui, OperationScope::Global);
        // Selection Operations (disabled if no selection)
        ui.add_enabled_ui(has_selection, |ui| {
            self.show_operations_submenu(ui, OperationScope::Selection);
        });
    }

    /// Renders the Convert Case + Line Operations submenus for a given scope.
    fn show_operations_submenu(&mut self, ui: &mut egui::Ui, scope: OperationScope) {
        let label = match scope {
            OperationScope::Global => "Global Operations",
            OperationScope::Selection => "Selection Operations",
        };
        ui.menu_button(label, |ui| {
            self.show_convert_case_submenu_scoped(ui, scope);
            self.show_line_operations_submenu_scoped(ui, scope);
        });
    }

    /// Convert Case submenu parameterized by scope.
    fn show_convert_case_submenu_scoped(&mut self, ui: &mut egui::Ui, scope: OperationScope) {
        ui.menu_button("Convert Case", |ui| {
            if ui.button("UPPERCASE").clicked() {
                self.convert_case_scoped(CaseConversion::Upper, scope);
                ui.close();
            }
            if ui.button("lowercase").clicked() {
                self.convert_case_scoped(CaseConversion::Lower, scope);
                ui.close();
            }
            if ui.button("Title Case").clicked() {
                self.convert_case_scoped(CaseConversion::TitleCase, scope);
                ui.close();
            }
        });
    }

    /// Line Operations submenu parameterized by scope.
    fn show_line_operations_submenu_scoped(&mut self, ui: &mut egui::Ui, scope: OperationScope) {
        ui.menu_button("Line Operations", |ui| {
            if ui.button("Duplicate Line").clicked() {
                self.duplicate_current_line();
                ui.close();
            }
            if ui.button("Move Line Up").clicked() {
                self.move_current_line_up();
                ui.close();
            }
            if ui.button("Move Line Down").clicked() {
                self.move_current_line_down();
                ui.close();
            }
            ui.separator();
            if ui.button("Sort Lines Ascending").clicked() {
                self.sort_lines_scoped(SortOrder::Ascending, scope);
                ui.close();
            }
            if ui.button("Sort Lines Descending").clicked() {
                self.sort_lines_scoped(SortOrder::Descending, scope);
                ui.close();
            }
            ui.separator();
            if ui.button("Remove Duplicate Lines").clicked() {
                self.remove_duplicate_lines_scoped(scope);
                ui.close();
            }
            if ui.button("Remove Empty Lines").clicked() {
                self.remove_empty_lines_scoped(scope);
                ui.close();
            }
        });
    }
}
