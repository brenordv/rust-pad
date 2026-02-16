//! Menu bar rendering for the editor application.
//!
//! Contains the File, Edit, Search, View, Encoding, Window, and Help menus.

use eframe::egui;
use rust_pad_core::encoding::{LineEnding, TextEncoding};
use rust_pad_core::line_ops::{CaseConversion, SortOrder};

use super::{App, ThemeMode};

impl App {
    /// Renders the menu bar with File, Edit, Search, View, Encoding, and Help menus.
    pub(crate) fn show_menu_bar(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        egui::MenuBar::new().ui(ui, |ui| {
            // File menu
            ui.menu_button("File", |ui| {
                if ui.button("New                  Ctrl+N").clicked() {
                    self.new_tab();
                    ui.close();
                }
                if ui.button("Open...              Ctrl+O").clicked() {
                    self.open_file_dialog();
                    ui.close();
                }
                ui.separator();
                if ui.button("Save                 Ctrl+S").clicked() {
                    self.save_active();
                    ui.close();
                }
                if ui.button("Save As...     Ctrl+Shift+S").clicked() {
                    self.save_as_dialog();
                    ui.close();
                }
                ui.separator();
                if ui.button("Close Tab            Ctrl+W").clicked() {
                    self.request_close_tab(self.tabs.active);
                    ui.close();
                }
                ui.separator();
                if ui.button("Exit").clicked() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    ui.close();
                }
            });

            // Edit menu
            ui.menu_button("Edit", |ui| {
                let can_undo = self.tabs.active_doc().history.can_undo();
                let can_redo = self.tabs.active_doc().history.can_redo();

                if ui
                    .add_enabled(can_undo, egui::Button::new("Undo            Ctrl+Z"))
                    .clicked()
                {
                    self.tabs.active_doc_mut().undo();
                    ui.close();
                }
                if ui
                    .add_enabled(can_redo, egui::Button::new("Redo            Ctrl+Y"))
                    .clicked()
                {
                    self.tabs.active_doc_mut().redo();
                    ui.close();
                }
                ui.separator();
                if ui.button("Cut              Ctrl+X").clicked() {
                    self.cut();
                    ui.close();
                }
                if ui.button("Copy             Ctrl+C").clicked() {
                    self.copy();
                    ui.close();
                }
                if ui.button("Paste            Ctrl+V").clicked() {
                    self.paste();
                    ui.close();
                }
                ui.separator();
                if ui.button("Select All       Ctrl+A").clicked() {
                    let doc = self.tabs.active_doc_mut();
                    doc.cursor.select_all(&doc.buffer);
                    ui.close();
                }
                ui.separator();

                // Case conversion submenu
                ui.menu_button("Convert Case", |ui| {
                    if ui.button("UPPERCASE").clicked() {
                        self.convert_selection_case(CaseConversion::Upper);
                        ui.close();
                    }
                    if ui.button("lowercase").clicked() {
                        self.convert_selection_case(CaseConversion::Lower);
                        ui.close();
                    }
                    if ui.button("Title Case").clicked() {
                        self.convert_selection_case(CaseConversion::TitleCase);
                        ui.close();
                    }
                });

                // Line operations submenu
                ui.menu_button("Line Operations", |ui| {
                    if ui.button("Duplicate Line").clicked() {
                        self.duplicate_current_line();
                        ui.close();
                    }
                    if ui.button("Move Line Up     Alt+Up").clicked() {
                        self.move_current_line_up();
                        ui.close();
                    }
                    if ui.button("Move Line Down   Alt+Down").clicked() {
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

                ui.separator();
                if ui.button("Increase Indent  Tab").clicked() {
                    self.indent_selection(true);
                    ui.close();
                }
                if ui.button("Decrease Indent  Shift+Tab").clicked() {
                    self.indent_selection(false);
                    ui.close();
                }
            });

            // Search menu
            ui.menu_button("Search", |ui| {
                if ui.button("Find/Replace     Ctrl+H").clicked() {
                    self.find_replace.open();
                    ui.close();
                }
                if ui.button("Find             Ctrl+F").clicked() {
                    self.find_replace.open();
                    ui.close();
                }
                ui.separator();
                if ui.button("Go to Line       Ctrl+G").clicked() {
                    self.go_to_line.open();
                    ui.close();
                }
                ui.separator();
                if ui.button("Toggle Bookmark  Ctrl+F2").clicked() {
                    let line = self.tabs.active_doc().cursor.position.line;
                    self.bookmarks.toggle(line);
                    ui.close();
                }
                if ui.button("Next Bookmark    F2").clicked() {
                    self.goto_next_bookmark();
                    ui.close();
                }
                if ui.button("Prev Bookmark    Shift+F2").clicked() {
                    self.goto_prev_bookmark();
                    ui.close();
                }
                if ui.button("Clear All Bookmarks").clicked() {
                    self.bookmarks.clear();
                    ui.close();
                }
            });

            // View menu
            ui.menu_button("View", |ui| {
                if ui.button("Zoom In          Ctrl++").clicked() {
                    self.zoom_level = (self.zoom_level + 0.1).min(self.max_zoom_level);
                    ui.close();
                }
                if ui.button("Zoom Out         Ctrl+-").clicked() {
                    self.zoom_level = (self.zoom_level - 0.1).max(0.5);
                    ui.close();
                }
                if ui.button("Reset Zoom       Ctrl+0").clicked() {
                    self.zoom_level = 1.0;
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
                ui.menu_button("Theme", |ui| {
                    let ctx_clone = ctx.clone();

                    // "System" entry
                    if ui.radio(self.theme_mode.is_system(), "System").clicked() {
                        self.set_theme_mode(ThemeMode::system(), &ctx_clone);
                        ui.close();
                    }
                    ui.separator();

                    // Dynamic theme entries
                    let theme_names: Vec<String> = self
                        .available_themes
                        .iter()
                        .map(|t| t.name.clone())
                        .collect();
                    for name in theme_names {
                        let is_selected = !self.theme_mode.is_system() && self.theme_mode.0 == name;
                        if ui.radio(is_selected, &name).clicked() {
                            self.set_theme_mode(ThemeMode(name), &ctx_clone);
                            ui.close();
                        }
                    }
                });
            });

            // Encoding menu
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

            // Settings menu
            ui.menu_button("Settings", |ui| {
                if ui.button("Preferences...").clicked() {
                    self.settings_open = true;
                    ui.close();
                }
            });

            // Help menu
            ui.menu_button("Help", |ui| {
                if ui.button("About rust-pad").clicked() {
                    self.about_open = true;
                    ui.close();
                }
            });

            // Window menu
            ui.menu_button("Window", |ui| {
                if ui.button("New Tab              Ctrl+N").clicked() {
                    self.new_tab();
                    ui.close();
                }
                if ui.button("Close Tab            Ctrl+W").clicked() {
                    self.request_close_tab(self.tabs.active);
                    ui.close();
                }
                ui.separator();
                if ui.button("Next Tab           Ctrl+Tab").clicked() {
                    let next = (self.tabs.active + 1) % self.tabs.tab_count();
                    self.tabs.switch_to(next);
                    ui.close();
                }
                if ui.button("Previous Tab  Ctrl+Shift+Tab").clicked() {
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
        });
    }
}
