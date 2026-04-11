//! Menu bar rendering for the editor application.
//!
//! Contains the File, Edit, Search, Encoding, View, Settings, Window, and Help menus.

use eframe::egui;
use rust_pad_core::encoding::{LineEnding, TextEncoding};
use rust_pad_core::line_ops::{CaseConversion, SortOrder};

use super::context_menu::OperationScope;
use super::{App, ThemeMode};

impl App {
    /// Renders the menu bar with File, Edit, Search, Encoding, View, Settings, Window, and Help menus.
    pub(crate) fn show_menu_bar(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        egui::MenuBar::new().ui(ui, |ui| {
            self.show_file_menu(ui, ctx);
            self.show_edit_menu(ui);
            self.show_search_menu(ui);
            self.show_encoding_menu(ui);
            self.show_view_menu(ui, ctx);
            self.show_settings_menu(ui);
            self.show_window_menu(ui);
            self.show_help_menu(ui);
        });
    }

    fn show_file_menu(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.menu_button("File", |ui| {
            ui.set_min_width(220.0);
            if ui
                .add(egui::Button::new("New").shortcut_text("Ctrl+N"))
                .clicked()
            {
                self.new_tab();
                ui.close();
            }
            if ui
                .add(egui::Button::new("Open...").shortcut_text("Ctrl+O"))
                .clicked()
            {
                self.open_file_dialog();
                ui.close();
            }

            if self.recent_files.enabled {
                self.show_recent_files_submenu(ui);
            }

            ui.separator();
            if ui
                .add(egui::Button::new("Save").shortcut_text("Ctrl+S"))
                .clicked()
            {
                self.save_active();
                ui.close();
            }
            if ui
                .add(egui::Button::new("Save As...").shortcut_text("Ctrl+Shift+S"))
                .clicked()
            {
                self.save_as_dialog();
                ui.close();
            }
            if ui.button("Save a Copy As...").clicked() {
                self.save_copy_dialog();
                ui.close();
            }
            ui.separator();
            let has_file = self.tabs.active_doc().file_path.is_some();
            if ui
                .add_enabled(has_file, egui::Button::new("Reload from Disk"))
                .clicked()
            {
                self.request_reload_from_disk();
                ui.close();
            }
            ui.separator();
            let can_print = self.can_print_active();
            if ui
                .add_enabled(
                    can_print,
                    egui::Button::new("Print...").shortcut_text("Ctrl+P"),
                )
                .clicked()
            {
                self.request_print();
                ui.close();
            }
            if ui
                .add_enabled(can_print, egui::Button::new("Export as PDF..."))
                .clicked()
            {
                self.request_export_pdf();
                ui.close();
            }
            ui.separator();
            if ui
                .add(egui::Button::new("Close Tab").shortcut_text("Ctrl+W"))
                .clicked()
            {
                self.request_close_tab(self.tabs.active);
                ui.close();
            }
            if ui.button("Close Unchanged Tabs").clicked() {
                self.close_unchanged_tabs();
                ui.close();
            }
            if ui.button("Close All Tabs").clicked() {
                self.close_all_tabs();
                ui.close();
            }
            ui.separator();
            if ui.button("Exit").clicked() {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                ui.close();
            }
        });
    }

    fn show_recent_files_submenu(&mut self, ui: &mut egui::Ui) {
        ui.menu_button("Open Recent", |ui| {
            ui.set_min_width(220.0);

            // Filter dead files if cleanup mode requires it
            self.recent_files.cleanup_on_menu_open();

            if self.recent_files.files.is_empty() {
                ui.add_enabled(false, egui::Button::new("No Recent Files"));
            } else {
                // Clone paths to avoid borrow issues
                let paths: Vec<std::path::PathBuf> = self.recent_files.files.clone();
                for path in &paths {
                    let file_name = path
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_else(|| path.to_string_lossy().into_owned());
                    let full_path = path.to_string_lossy().into_owned();
                    if ui.button(&file_name).on_hover_text(&full_path).clicked() {
                        self.open_file_path(path);
                        ui.close();
                    }
                }
                ui.separator();
                if ui.button("Clear Recent Files List").clicked() {
                    self.recent_files.files.clear();
                    ui.close();
                }
            }
        });
    }

    fn show_edit_menu(&mut self, ui: &mut egui::Ui) {
        ui.menu_button("Edit", |ui| {
            ui.set_min_width(220.0);
            let can_undo = self.tabs.active_doc().history.can_undo();
            let can_redo = self.tabs.active_doc().history.can_redo();

            if ui
                .add_enabled(can_undo, egui::Button::new("Undo").shortcut_text("Ctrl+Z"))
                .clicked()
            {
                self.tabs.active_doc_mut().undo();
                ui.close();
            }
            if ui
                .add_enabled(can_redo, egui::Button::new("Redo").shortcut_text("Ctrl+Y"))
                .clicked()
            {
                self.tabs.active_doc_mut().redo();
                ui.close();
            }
            ui.separator();
            if ui
                .add(egui::Button::new("Cut").shortcut_text("Ctrl+X"))
                .clicked()
            {
                self.cut();
                ui.close();
            }
            if ui
                .add(egui::Button::new("Copy").shortcut_text("Ctrl+C"))
                .clicked()
            {
                self.copy();
                ui.close();
            }
            if ui
                .add(egui::Button::new("Paste").shortcut_text("Ctrl+V"))
                .clicked()
            {
                self.paste();
                ui.close();
            }
            ui.separator();
            if ui
                .add(egui::Button::new("Select All").shortcut_text("Ctrl+A"))
                .clicked()
            {
                let doc = self.tabs.active_doc_mut();
                doc.cursor.select_all(&doc.buffer);
                ui.close();
            }
            ui.separator();

            if ui
                .add(egui::Button::new("Find/Replace").shortcut_text("Ctrl+H"))
                .clicked()
            {
                self.find_replace.open();
                ui.close();
            }
            ui.separator();

            self.show_convert_case_submenu(ui);
            self.show_line_operations_submenu(ui);

            ui.separator();
            if ui
                .add(egui::Button::new("Increase Indent").shortcut_text("Tab"))
                .clicked()
            {
                self.indent_selection(true);
                ui.close();
            }
            if ui
                .add(egui::Button::new("Decrease Indent").shortcut_text("Shift+Tab"))
                .clicked()
            {
                self.indent_selection(false);
                ui.close();
            }
        });
    }

    fn show_convert_case_submenu(&mut self, ui: &mut egui::Ui) {
        ui.menu_button("Convert Case", |ui| {
            if ui.button("UPPERCASE").clicked() {
                self.convert_case_scoped(CaseConversion::Upper, OperationScope::Selection);
                ui.close();
            }
            if ui.button("lowercase").clicked() {
                self.convert_case_scoped(CaseConversion::Lower, OperationScope::Selection);
                ui.close();
            }
            if ui.button("Title Case").clicked() {
                self.convert_case_scoped(CaseConversion::TitleCase, OperationScope::Selection);
                ui.close();
            }
        });
    }

    fn show_line_operations_submenu(&mut self, ui: &mut egui::Ui) {
        ui.menu_button("Line Operations", |ui| {
            ui.set_min_width(220.0);
            if ui.button("Duplicate Line").clicked() {
                self.duplicate_current_line();
                ui.close();
            }
            if ui
                .add(egui::Button::new("Move Line Up").shortcut_text("Alt+Up"))
                .clicked()
            {
                self.move_current_line_up();
                ui.close();
            }
            if ui
                .add(egui::Button::new("Move Line Down").shortcut_text("Alt+Down"))
                .clicked()
            {
                self.move_current_line_down();
                ui.close();
            }
            ui.separator();
            if ui.button("Sort Lines Ascending").clicked() {
                self.sort_lines(SortOrder::Ascending);
                ui.close();
            }
            if ui.button("Sort Lines Descending").clicked() {
                self.sort_lines(SortOrder::Descending);
                ui.close();
            }
            ui.separator();
            if ui.button("Remove Duplicate Lines").clicked() {
                self.remove_duplicate_lines();
                ui.close();
            }
            if ui.button("Remove Empty Lines").clicked() {
                self.remove_empty_lines();
                ui.close();
            }
        });
    }

    fn show_search_menu(&mut self, ui: &mut egui::Ui) {
        ui.menu_button("Search", |ui| {
            ui.set_min_width(220.0);
            if ui
                .add(egui::Button::new("Find/Replace").shortcut_text("Ctrl+H"))
                .clicked()
            {
                self.find_replace.open();
                ui.close();
            }
            if ui
                .add(egui::Button::new("Find").shortcut_text("Ctrl+F"))
                .clicked()
            {
                self.find_replace.open();
                ui.close();
            }
            ui.separator();
            if ui
                .add(egui::Button::new("Go to Line").shortcut_text("Ctrl+G"))
                .clicked()
            {
                self.go_to_line.open();
                ui.close();
            }
            ui.separator();
            if ui
                .add(egui::Button::new("Toggle Bookmark").shortcut_text("Ctrl+F2"))
                .clicked()
            {
                let line = self.tabs.active_doc().cursor.position.line;
                self.bookmarks.toggle(line);
                ui.close();
            }
            if ui
                .add(egui::Button::new("Next Bookmark").shortcut_text("F2"))
                .clicked()
            {
                self.goto_next_bookmark();
                ui.close();
            }
            if ui
                .add(egui::Button::new("Prev Bookmark").shortcut_text("Shift+F2"))
                .clicked()
            {
                self.goto_prev_bookmark();
                ui.close();
            }
            if ui.button("Clear All Bookmarks").clicked() {
                self.bookmarks.clear();
                ui.close();
            }
        });
    }

    fn show_encoding_menu(&mut self, ui: &mut egui::Ui) {
        ui.menu_button("Encoding", |ui| {
            let current = self.tabs.active_doc().encoding;
            let current_eol = self.tabs.active_doc().line_ending;
            ui.label(format!("Current: {current}"));
            ui.separator();

            for enc in [
                TextEncoding::Utf8,
                TextEncoding::Utf8Bom,
                TextEncoding::Utf16Le,
                TextEncoding::Utf16Be,
                TextEncoding::Ascii,
            ] {
                if ui.radio(current == enc, format!("{enc}")).clicked() {
                    self.tabs.active_doc_mut().encoding = enc;
                    self.tabs.active_doc_mut().modified = true;
                    ui.close();
                }
            }

            ui.separator();
            ui.label(format!("Line Ending: {current_eol}"));
            for eol in [LineEnding::Lf, LineEnding::CrLf, LineEnding::Cr] {
                if ui.radio(current_eol == eol, format!("{eol}")).clicked() {
                    self.tabs.active_doc_mut().line_ending = eol;
                    self.tabs.active_doc_mut().modified = true;
                    ui.close();
                }
            }
        });
    }

    fn show_view_menu(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.menu_button("View", |ui| {
            ui.set_min_width(220.0);
            if ui
                .add(egui::Button::new("Zoom In").shortcut_text("Ctrl++"))
                .clicked()
            {
                self.theme_ctrl.zoom_in();
                ui.close();
            }
            if ui
                .add(egui::Button::new("Zoom Out").shortcut_text("Ctrl+-"))
                .clicked()
            {
                self.theme_ctrl.zoom_out();
                ui.close();
            }
            if ui
                .add(egui::Button::new("Reset Zoom").shortcut_text("Ctrl+0"))
                .clicked()
            {
                self.theme_ctrl.zoom_reset();
                ui.close();
            }
            ui.separator();
            if ui.checkbox(&mut self.word_wrap, "Word Wrap").clicked() {
                ui.close();
            }
            if ui
                .checkbox(&mut self.show_special_chars, "Show Special Characters")
                .clicked()
            {
                ui.close();
            }
            if ui
                .checkbox(&mut self.show_line_numbers, "Show Line Numbers")
                .clicked()
            {
                ui.close();
            }
            if ui
                .checkbox(&mut self.restore_open_files, "Restore Files on Startup")
                .clicked()
            {
                ui.close();
            }
            if ui
                .checkbox(&mut self.show_full_path_in_title, "Show Full Path in Title")
                .clicked()
            {
                ui.close();
            }
            ui.separator();
            let has_file = self.tabs.active_doc().file_path.is_some();
            let mut live_monitoring = self.tabs.active_doc().live_monitoring;
            let monitoring_response = ui.add_enabled(
                has_file,
                egui::Checkbox::new(&mut live_monitoring, "Live File Monitoring"),
            );
            if monitoring_response.clicked() {
                self.tabs.active_doc_mut().live_monitoring = live_monitoring;
                ui.close();
            }
            ui.separator();
            // ── Split view ────────────────────────────────────────────
            if ui
                .add(egui::Button::new("Split Vertically").shortcut_text("Ctrl+Alt+V"))
                .clicked()
            {
                self.toggle_split_vertical();
                ui.close();
            }
            if ui
                .add(egui::Button::new("Split Horizontally").shortcut_text("Ctrl+Alt+H"))
                .clicked()
            {
                self.toggle_split_horizontal();
                ui.close();
            }
            let split_active = self.is_split();
            if ui
                .add_enabled(split_active, egui::Button::new("Remove Split"))
                .clicked()
            {
                self.remove_split();
                ui.close();
            }
            // Synchronized scrolling — only meaningful when split is active.
            // The flag itself is persisted in AppConfig regardless, so
            // disabling/re-enabling the split preserves the user's choice.
            let sync_button = egui::Button::new(if self.sync_scroll_enabled {
                "✓ Synchronized Scrolling"
            } else {
                "  Synchronized Scrolling"
            })
            .shortcut_text("Ctrl+Alt+S");
            let sync_response = ui.add_enabled(split_active, sync_button);
            if !split_active {
                sync_response.on_hover_text("Enable Split View first");
            } else if sync_response.clicked() {
                self.toggle_sync_scroll();
                ui.close();
            }
            ui.separator();
            self.show_theme_submenu(ui, ctx);
        });
    }

    fn show_theme_submenu(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.menu_button("Theme", |ui| {
            let ctx_clone = ctx.clone();

            // "System" entry
            if ui
                .radio(self.theme_ctrl.theme_mode.is_system(), "System")
                .clicked()
            {
                self.theme_ctrl.set_mode(ThemeMode::system(), &ctx_clone);
                ui.close();
            }
            ui.separator();

            // Dynamic theme entries
            let theme_names: Vec<String> = self
                .theme_ctrl
                .available_themes
                .iter()
                .map(|t| t.name.clone())
                .collect();
            for name in theme_names {
                let is_selected =
                    !self.theme_ctrl.theme_mode.is_system() && self.theme_ctrl.theme_mode.0 == name;
                if ui.radio(is_selected, &name).clicked() {
                    self.theme_ctrl.set_mode(ThemeMode(name), &ctx_clone);
                    ui.close();
                }
            }
        });
    }

    fn show_settings_menu(&mut self, ui: &mut egui::Ui) {
        ui.menu_button("Settings", |ui| {
            if ui.button("Preferences...").clicked() {
                self.settings_open = true;
                ui.close();
            }
        });
    }

    fn show_window_menu(&mut self, ui: &mut egui::Ui) {
        ui.menu_button("Window", |ui| {
            ui.set_min_width(220.0);
            if ui
                .add(egui::Button::new("New Tab").shortcut_text("Ctrl+N"))
                .clicked()
            {
                self.new_tab();
                ui.close();
            }
            if ui
                .add(egui::Button::new("Close Tab").shortcut_text("Ctrl+W"))
                .clicked()
            {
                self.request_close_tab(self.tabs.active);
                ui.close();
            }
            ui.separator();
            if ui
                .add(egui::Button::new("Next Tab").shortcut_text("Ctrl+Tab"))
                .clicked()
            {
                let next = (self.tabs.active + 1) % self.tabs.tab_count();
                self.tabs.switch_to(next);
                ui.close();
            }
            if ui
                .add(egui::Button::new("Previous Tab").shortcut_text("Ctrl+Shift+Tab"))
                .clicked()
            {
                let count = self.tabs.tab_count();
                let prev = (self.tabs.active + count - 1) % count;
                self.tabs.switch_to(prev);
                ui.close();
            }
            ui.separator();

            // List of open tabs
            let tab_count = self.tabs.tab_count();
            let active = self.tabs.active;
            for idx in 0..tab_count {
                let title = &self.tabs.documents[idx].title;
                let modified = self.tabs.documents[idx].modified;
                let label = if modified {
                    format!("{} {title} *", idx + 1)
                } else {
                    format!("{} {title}", idx + 1)
                };
                if ui.radio(idx == active, label).clicked() {
                    self.tabs.switch_to(idx);
                    ui.close();
                }
            }
        });
    }

    fn show_help_menu(&mut self, ui: &mut egui::Ui) {
        ui.menu_button("Help", |ui| {
            if ui.button("About rust-pad").clicked() {
                self.about_open = true;
                ui.close();
            }
        });
    }
}
