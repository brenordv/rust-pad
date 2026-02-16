//! Tab bar rendering for the editor application.
//!
//! Handles the tab strip with active tab highlighting, close buttons,
//! context menus, middle-click close, and new tab creation.

use eframe::egui;
use egui::{Color32, RichText, Stroke, Visuals};

use super::App;

impl App {
    /// Renders the tab bar with active tab highlighting and close buttons.
    pub(crate) fn show_tab_bar(&mut self, ui: &mut egui::Ui) {
        let visuals = ui.visuals().clone();

        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            let mut tab_to_close: Option<usize> = None;

            for idx in 0..self.tabs.tab_count() {
                self.render_tab_button(ui, idx, &visuals, &mut tab_to_close);
            }

            self.render_new_tab_button(ui, &visuals);
            self.render_empty_tab_bar_area(ui);

            if let Some(idx) = tab_to_close {
                self.request_close_tab(idx);
            }
        });
    }

    /// Renders a single tab button with its label, close button, accent line,
    /// separator, and context menu.
    fn render_tab_button(
        &mut self,
        ui: &mut egui::Ui,
        idx: usize,
        visuals: &Visuals,
        tab_to_close: &mut Option<usize>,
    ) {
        let doc = &self.tabs.documents[idx];
        let is_active = idx == self.tabs.active;

        let title = if doc.modified {
            format!("{} *", doc.title)
        } else {
            doc.title.clone()
        };

        let padded_title = format!("  {title}  ");
        let text = if is_active {
            RichText::new(&padded_title).color(if visuals.dark_mode {
                Color32::from_rgb(220, 220, 220)
            } else {
                Color32::from_rgb(30, 30, 30)
            })
        } else {
            RichText::new(&padded_title).color(visuals.widgets.noninteractive.fg_stroke.color)
        };

        let fill = if is_active {
            visuals.widgets.active.bg_fill
        } else {
            visuals.faint_bg_color
        };

        let button = egui::Button::new(text)
            .fill(fill)
            .corner_radius(egui::CornerRadius {
                nw: 4,
                ne: 4,
                sw: 0,
                se: 0,
            })
            .stroke(Stroke::NONE)
            .min_size(egui::Vec2::new(0.0, 32.0));

        let response = ui.add(button);

        // Draw teal accent line on active tab
        if is_active {
            let tab_rect = response.rect;
            ui.painter().line_segment(
                [
                    egui::Pos2::new(tab_rect.min.x, tab_rect.min.y),
                    egui::Pos2::new(tab_rect.max.x, tab_rect.min.y),
                ],
                Stroke::new(2.0, self.accent_color),
            );
        }

        if response.clicked() {
            self.tabs.switch_to(idx);
        }

        if response.middle_clicked() {
            *tab_to_close = Some(idx);
        }

        // Close button on active tab
        if is_active {
            self.render_tab_close_button(ui, idx, visuals, tab_to_close);
        }

        // 1px separator between tabs
        if idx < self.tabs.tab_count() - 1 {
            let tab_rect = response.rect;
            ui.painter().line_segment(
                [
                    egui::Pos2::new(tab_rect.max.x, tab_rect.min.y + 4.0),
                    egui::Pos2::new(tab_rect.max.x, tab_rect.max.y - 4.0),
                ],
                Stroke::new(1.0, visuals.widgets.noninteractive.bg_stroke.color),
            );
        }

        self.render_tab_context_menu(ui, idx, &response, tab_to_close);
    }

    /// Renders the close button ("x") on the active tab.
    fn render_tab_close_button(
        &self,
        ui: &mut egui::Ui,
        idx: usize,
        visuals: &Visuals,
        tab_to_close: &mut Option<usize>,
    ) {
        let close_text = RichText::new("\u{00D7}") // x
            .color(visuals.widgets.noninteractive.fg_stroke.color)
            .size(14.0);
        let close_btn = egui::Button::new(close_text)
            .fill(Color32::TRANSPARENT)
            .stroke(Stroke::NONE);
        let close_response = ui.add(close_btn);
        if close_response.clicked() {
            *tab_to_close = Some(idx);
        }
        // Highlight close button on hover
        if close_response.hovered() {
            ui.painter()
                .rect_filled(close_response.rect, 2.0, visuals.widgets.hovered.bg_fill);
        }
    }

    /// Renders the right-click context menu for a tab.
    fn render_tab_context_menu(
        &mut self,
        _ui: &mut egui::Ui,
        idx: usize,
        response: &egui::Response,
        tab_to_close: &mut Option<usize>,
    ) {
        response.context_menu(|ui| {
            if ui.button("Close").clicked() {
                *tab_to_close = Some(idx);
                ui.close();
            }
            if ui.button("Close Others").clicked() {
                let mut i = self.tabs.tab_count();
                while i > 0 {
                    i -= 1;
                    if i != idx {
                        self.cleanup_session_for_tab(i);
                        self.tabs.close_tab(i);
                    }
                }
                self.tabs.active = 0;
                ui.close();
            }
        });
    }

    /// Renders the "+" button for creating a new tab.
    fn render_new_tab_button(&mut self, ui: &mut egui::Ui, visuals: &Visuals) {
        ui.spacing_mut().item_spacing.x = 4.0;
        let new_btn = egui::Button::new(
            RichText::new("+")
                .color(visuals.widgets.noninteractive.fg_stroke.color)
                .size(16.0),
        )
        .fill(Color32::TRANSPARENT)
        .stroke(Stroke::NONE);
        if ui.add(new_btn).clicked() {
            self.new_tab();
        }
    }

    /// Handles double-click on the empty tab bar area to create a new tab.
    fn render_empty_tab_bar_area(&mut self, ui: &mut egui::Ui) {
        let remaining = ui.available_size();
        if remaining.x > 0.0 {
            let empty_response = ui.allocate_response(remaining, egui::Sense::click());
            if empty_response.double_clicked() {
                self.new_tab();
            }
        }
    }
}
