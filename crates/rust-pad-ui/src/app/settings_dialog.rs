//! In-app settings dialog for configuring all application preferences.
//!
//! Provides a two-column interface: a left navigation sidebar and a right content panel.

use eframe::egui;
use rust_pad_config::RecentFilesCleanup;

use super::App;

/// Which section of the settings dialog is currently visible.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum SettingsTab {
    #[default]
    General,
    Editor,
    FileDialogs,
    AutoSave,
    History,
}

/// Fixed width for the navigation sidebar (in logical pixels).
const SIDEBAR_WIDTH: f32 = 130.0;

impl App {
    /// Renders the settings dialog window.
    ///
    /// Returns `true` if the dialog is open (for dialog gating).
    pub(crate) fn show_settings_dialog(&mut self, ctx: &egui::Context) -> bool {
        if !self.settings_open {
            return false;
        }

        let mut open = true;
        egui::Window::new("Settings")
            .collapsible(false)
            .resizable(true)
            .default_size([620.0, 420.0])
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .open(&mut open)
            .show(ctx, |ui| {
                // Pin both panels to the full available height so the window
                // stays the same size regardless of which section is active.
                let panel_height = ui.available_height();

                ui.horizontal_top(|ui| {
                    // ── Left navigation sidebar ──────────────────────
                    ui.allocate_ui_with_layout(
                        egui::vec2(SIDEBAR_WIDTH, panel_height),
                        egui::Layout::top_down_justified(egui::Align::LEFT),
                        |ui| {
                            ui.selectable_value(
                                &mut self.settings_tab,
                                SettingsTab::General,
                                "General",
                            );
                            ui.selectable_value(
                                &mut self.settings_tab,
                                SettingsTab::Editor,
                                "Editor",
                            );
                            ui.selectable_value(
                                &mut self.settings_tab,
                                SettingsTab::FileDialogs,
                                "File Dialogs",
                            );
                            ui.selectable_value(
                                &mut self.settings_tab,
                                SettingsTab::AutoSave,
                                "Auto-Save",
                            );
                            ui.selectable_value(
                                &mut self.settings_tab,
                                SettingsTab::History,
                                "History",
                            );
                        },
                    );

                    ui.separator();

                    // ── Right content panel ──────────────────────────
                    // Explicit top-down layout prevents inheriting the
                    // horizontal direction from the parent.
                    ui.allocate_ui_with_layout(
                        egui::vec2(ui.available_width(), panel_height),
                        egui::Layout::top_down(egui::Align::LEFT),
                        |ui| {
                            egui::ScrollArea::vertical().show(ui, |ui| match self.settings_tab {
                                SettingsTab::General => self.settings_general(ui, ctx),
                                SettingsTab::Editor => self.settings_editor(ui),
                                SettingsTab::FileDialogs => self.settings_file_dialogs(ui),
                                SettingsTab::AutoSave => self.settings_auto_save(ui),
                                SettingsTab::History => self.settings_history(ui),
                            });
                        },
                    );
                });
            });

        if !open {
            self.settings_open = false;
        }

        self.settings_open
    }

    fn settings_general(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.heading("General");
        ui.add_space(4.0);

        ui.checkbox(&mut self.restore_open_files, "Restore files on startup");
        ui.checkbox(
            &mut self.show_full_path_in_title,
            "Show full path in title bar",
        );

        ui.add_space(8.0);
        ui.separator();
        ui.add_space(4.0);

        ui.heading("Theme");
        ui.add_space(4.0);

        let theme_names: Vec<String> = std::iter::once("System".to_string())
            .chain(
                self.theme_ctrl
                    .available_themes
                    .iter()
                    .map(|t| t.name.clone()),
            )
            .collect();

        let current_label = self.theme_ctrl.theme_mode.0.clone();
        egui::ComboBox::from_label("Theme")
            .selected_text(&current_label)
            .show_ui(ui, |ui| {
                let ctx_clone = ctx.clone();
                for name in &theme_names {
                    if ui
                        .selectable_value(&mut self.theme_ctrl.theme_mode.0, name.clone(), name)
                        .changed()
                    {
                        self.theme_ctrl.set_mode(
                            super::ThemeMode(self.theme_ctrl.theme_mode.0.clone()),
                            &ctx_clone,
                        );
                    }
                }
            });

        ui.add_space(8.0);
        ui.separator();
        ui.add_space(4.0);

        ui.heading("Font");
        ui.add_space(4.0);

        ui.horizontal(|ui| {
            ui.label("Font size:");
            ui.add(egui::DragValue::new(&mut self.theme_ctrl.theme.font_size).range(6.0..=72.0));
        });

        ui.horizontal(|ui| {
            ui.label("Max zoom level:");
            ui.add(egui::DragValue::new(&mut self.theme_ctrl.max_zoom_level).range(1.0..=50.0));
        });
    }

    fn settings_editor(&mut self, ui: &mut egui::Ui) {
        ui.heading("Editor");
        ui.add_space(4.0);

        ui.checkbox(&mut self.word_wrap, "Word wrap");
        ui.checkbox(&mut self.show_special_chars, "Show special characters");
        ui.checkbox(&mut self.show_line_numbers, "Show line numbers");

        ui.add_space(8.0);
        ui.separator();
        ui.add_space(4.0);

        ui.heading("Default Extension");
        ui.add_space(4.0);

        ui.horizontal(|ui| {
            ui.label("Extension for new tabs:");
            let response = ui.add(
                egui::TextEdit::singleline(&mut self.file_dialog.default_extension)
                    .desired_width(80.0),
            );
            if response.changed() {
                self.tabs.default_extension = self.file_dialog.default_extension.clone();
            }
        });
        ui.label(
            egui::RichText::new("Leave empty for no extension. Examples: txt, md, rs")
                .small()
                .color(egui::Color32::GRAY),
        );
    }

    fn settings_file_dialogs(&mut self, ui: &mut egui::Ui) {
        ui.heading("File Dialogs");
        ui.add_space(4.0);

        ui.checkbox(
            &mut self.file_dialog.remember_last_folder,
            "Remember last used folder",
        );

        ui.add_space(8.0);

        ui.horizontal(|ui| {
            ui.label("Default work folder:");
            ui.add(
                egui::TextEdit::singleline(&mut self.file_dialog.default_work_folder)
                    .desired_width(300.0),
            );
        });
        ui.label(
            egui::RichText::new("Leave empty to use the home directory")
                .small()
                .color(egui::Color32::GRAY),
        );
    }

    fn settings_auto_save(&mut self, ui: &mut egui::Ui) {
        ui.heading("Auto-Save");
        ui.add_space(4.0);

        ui.checkbox(
            &mut self.auto_save.enabled,
            "Enable auto-save for file-backed documents",
        );
        ui.label(
            egui::RichText::new("Only saves files that already exist on disk")
                .small()
                .color(egui::Color32::GRAY),
        );

        ui.add_space(8.0);

        ui.add_enabled_ui(self.auto_save.enabled, |ui| {
            ui.horizontal(|ui| {
                ui.label("Save interval (seconds):");
                let mut interval = self.auto_save.interval_secs as f64;
                if ui
                    .add(egui::DragValue::new(&mut interval).range(5.0..=3600.0))
                    .changed()
                {
                    self.auto_save.interval_secs = (interval as u64).max(5);
                }
            });
        });
    }

    fn settings_history(&mut self, ui: &mut egui::Ui) {
        ui.heading("Recent Files");
        ui.add_space(4.0);

        ui.checkbox(
            &mut self.recent_files.enabled,
            "Enable recent files history",
        );

        ui.add_space(8.0);

        ui.add_enabled_ui(self.recent_files.enabled, |ui| {
            ui.horizontal(|ui| {
                ui.label("Max files to show:");
                let mut count = self.recent_files.max_count as f64;
                if ui
                    .add(egui::DragValue::new(&mut count).range(1.0..=50.0))
                    .changed()
                {
                    self.recent_files.max_count = (count as usize).clamp(1, 50);
                    self.recent_files
                        .files
                        .truncate(self.recent_files.max_count);
                }
            });

            ui.add_space(8.0);

            ui.horizontal(|ui| {
                ui.label("Remove unavailable files:");
                egui::ComboBox::from_id_salt("recent_files_cleanup")
                    .selected_text(match self.recent_files.cleanup {
                        RecentFilesCleanup::OnStartup => "On Startup",
                        RecentFilesCleanup::OnMenuOpen => "When Menu Opens",
                        RecentFilesCleanup::Both => "Both",
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut self.recent_files.cleanup,
                            RecentFilesCleanup::OnStartup,
                            "On Startup",
                        );
                        ui.selectable_value(
                            &mut self.recent_files.cleanup,
                            RecentFilesCleanup::OnMenuOpen,
                            "When Menu Opens",
                        );
                        ui.selectable_value(
                            &mut self.recent_files.cleanup,
                            RecentFilesCleanup::Both,
                            "Both",
                        );
                    });
            });

            ui.add_space(8.0);

            if ui.button("Clear Recent Files List Now").clicked() {
                self.recent_files.files.clear();
            }
        });
    }
}
