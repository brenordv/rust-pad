//! Problems dialog showing application error log entries.
//!
//! Entries are stored in a crash-safe redb database and survive unexpected
//! termination. Users can mark individual entries or all entries as read,
//! and clear the entire log.

use eframe::egui;

use super::App;

impl App {
    /// Renders the Problems dialog window.
    pub(crate) fn show_problems_dialog(&mut self, ctx: &egui::Context) {
        if !self.problems_open {
            return;
        }

        let mut open = true;
        let mut action = ProblemAction::None;

        egui::Window::new("Problems")
            .collapsible(false)
            .resizable(true)
            .default_width(520.0)
            .default_height(400.0)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .open(&mut open)
            .show(ctx, |ui| {
                let entries = crate::problem_log::store()
                    .and_then(|s| s.load_all().ok())
                    .unwrap_or_default();

                // Toolbar
                ui.horizontal(|ui| {
                    let unread = entries.iter().filter(|e| !e.read).count();
                    ui.label(format!("{} total, {} unread", entries.len(), unread));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add_enabled(!entries.is_empty(), egui::Button::new("Clear All"))
                            .clicked()
                        {
                            action = ProblemAction::ClearAll;
                        }
                        if ui
                            .add_enabled(unread > 0, egui::Button::new("Mark All Read"))
                            .clicked()
                        {
                            action = ProblemAction::MarkAllRead;
                        }
                    });
                });

                ui.separator();

                if entries.is_empty() {
                    ui.vertical_centered(|ui| {
                        ui.add_space(40.0);
                        ui.label("No problems recorded.");
                        ui.add_space(40.0);
                    });
                } else {
                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            for entry in &entries {
                                let datetime = chrono::DateTime::from_timestamp(entry.timestamp, 0)
                                    .map(|dt| {
                                        dt.with_timezone(&chrono::Local)
                                            .format("%Y-%m-%d %H:%M:%S")
                                            .to_string()
                                    })
                                    .unwrap_or_else(|| "Unknown time".to_string());

                                let frame = egui::Frame::NONE
                                    .inner_margin(egui::Margin::same(6))
                                    .corner_radius(egui::CornerRadius::same(4))
                                    .fill(if entry.read {
                                        ui.visuals().faint_bg_color
                                    } else {
                                        ui.visuals().extreme_bg_color
                                    });

                                frame.show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        if !entry.read {
                                            ui.colored_label(
                                                ui.visuals().warn_fg_color,
                                                "\u{25CF}",
                                            );
                                        }

                                        ui.vertical(|ui| {
                                            ui.horizontal(|ui| {
                                                ui.label(
                                                    egui::RichText::new(&datetime).small().weak(),
                                                );

                                                ui.with_layout(
                                                    egui::Layout::right_to_left(
                                                        egui::Align::Center,
                                                    ),
                                                    |ui| {
                                                        if !entry.read
                                                            && ui
                                                                .small_button("Mark Read")
                                                                .clicked()
                                                        {
                                                            action =
                                                                ProblemAction::MarkRead(entry.id);
                                                        }
                                                        if ui.small_button("Copy").clicked() {
                                                            action = ProblemAction::CopyText(
                                                                entry.message.clone(),
                                                            );
                                                        }
                                                    },
                                                );
                                            });

                                            ui.label(&entry.message);
                                        });
                                    });
                                });

                                ui.add_space(2.0);
                            }
                        });
                }
            });

        // Process actions after the UI pass to avoid borrow conflicts.
        match action {
            ProblemAction::None => {}
            ProblemAction::MarkRead(id) => {
                if let Some(store) = crate::problem_log::store() {
                    if let Err(e) = store.mark_as_read(id) {
                        tracing::warn!("Failed to mark problem as read: {e}");
                    }
                }
                self.refresh_problem_count();
            }
            ProblemAction::MarkAllRead => {
                if let Some(store) = crate::problem_log::store() {
                    if let Err(e) = store.mark_all_as_read() {
                        tracing::warn!("Failed to mark all problems as read: {e}");
                    }
                }
                self.refresh_problem_count();
            }
            ProblemAction::CopyText(text) => {
                if let Some(ref mut clipboard) = self.clipboard {
                    let _ = clipboard.set_text(text);
                }
            }
            ProblemAction::ClearAll => {
                if let Some(store) = crate::problem_log::store() {
                    if let Err(e) = store.clear_all() {
                        tracing::warn!("Failed to clear problem log: {e}");
                    }
                }
                self.refresh_problem_count();
            }
        }

        if !open {
            self.problems_open = false;
        }
    }
}

/// Deferred action from the problems dialog UI.
enum ProblemAction {
    None,
    MarkRead(u64),
    CopyText(String),
    MarkAllRead,
    ClearAll,
}
