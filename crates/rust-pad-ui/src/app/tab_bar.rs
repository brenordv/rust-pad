//! Tab bar rendering for the editor application.
//!
//! Handles the tab strip with active tab highlighting, close buttons,
//! context menus, middle-click close, and new tab creation.

use eframe::egui;
use egui::{Color32, Rect, RichText, Sense, Stroke, Vec2, Visuals};

use super::App;

/// Horizontal padding on each side of the tab content.
const TAB_PADDING: f32 = 8.0;
/// Gap between the title text and the close button area.
const TITLE_CLOSE_GAP: f32 = 4.0;
/// Side length of the square close button area.
const CLOSE_AREA_SIZE: f32 = 14.0;
/// Fixed tab height.
const TAB_HEIGHT: f32 = 32.0;

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

    /// Renders a single tab as a unified rect with painted title, close button,
    /// accent line, separator, and context menu.
    ///
    /// The tab width is: `pad + title_width + gap + close_area + pad`.
    /// The close button area is always reserved so that tab width stays constant.
    /// The close glyph is only drawn when the tab is active or hovered.
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

        // -- Measure title text --
        let title_color = if is_active {
            if visuals.dark_mode {
                Color32::from_rgb(220, 220, 220)
            } else {
                Color32::from_rgb(30, 30, 30)
            }
        } else {
            visuals.widgets.noninteractive.fg_stroke.color
        };

        let title_font = egui::FontId::proportional(14.0);
        let title_galley = ui
            .painter()
            .layout_no_wrap(title.clone(), title_font, title_color);
        let title_width = title_galley.size().x;

        // -- Calculate tab dimensions --
        let tab_width = TAB_PADDING + title_width + TITLE_CLOSE_GAP + CLOSE_AREA_SIZE + TAB_PADDING;
        let tab_size = Vec2::new(tab_width, TAB_HEIGHT);

        // -- Allocate the single rect for the entire tab --
        let (tab_rect, response) = ui.allocate_exact_size(tab_size, Sense::click_and_drag());
        response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &title));
        let is_hovered = response.hovered();
        let painter = ui.painter();

        // -- Paint background --
        let fill = if is_active {
            visuals.widgets.active.bg_fill
        } else if is_hovered {
            // Slight highlight on hover for inactive tabs
            visuals.widgets.hovered.weak_bg_fill
        } else {
            visuals.faint_bg_color
        };

        painter.rect_filled(
            tab_rect,
            egui::CornerRadius {
                nw: 4,
                ne: 4,
                sw: 0,
                se: 0,
            },
            fill,
        );

        // -- Paint accent line on active tab --
        if is_active {
            painter.line_segment(
                [
                    egui::Pos2::new(tab_rect.min.x, tab_rect.min.y),
                    egui::Pos2::new(tab_rect.max.x, tab_rect.min.y),
                ],
                Stroke::new(2.0, self.theme_ctrl.accent_color),
            );
        }

        // -- Paint title text (vertically centered, after left padding) --
        let title_pos = egui::Pos2::new(
            tab_rect.min.x + TAB_PADDING,
            tab_rect.center().y - title_galley.size().y / 2.0,
        );
        painter.galley(title_pos, title_galley, title_color);

        // -- Close button area (always at the same position) --
        let close_rect = Rect::from_min_size(
            egui::Pos2::new(
                tab_rect.max.x - TAB_PADDING - CLOSE_AREA_SIZE,
                tab_rect.center().y - CLOSE_AREA_SIZE / 2.0,
            ),
            Vec2::splat(CLOSE_AREA_SIZE),
        );

        let pointer_in_close = ui
            .input(|i| i.pointer.hover_pos())
            .is_some_and(|pos| close_rect.contains(pos));

        // Draw the close glyph when tab is active or hovered
        if is_active || is_hovered {
            // Highlight background on close button hover
            if pointer_in_close {
                painter.rect_filled(close_rect, 2.0, visuals.widgets.hovered.bg_fill);
            }

            let close_font = egui::FontId::proportional(14.0);
            let close_color = visuals.widgets.noninteractive.fg_stroke.color;
            let close_galley =
                painter.layout_no_wrap("\u{00D7}".to_owned(), close_font, close_color);
            let close_text_pos = egui::Pos2::new(
                close_rect.center().x - close_galley.size().x / 2.0,
                close_rect.center().y - close_galley.size().y / 2.0,
            );
            painter.galley(close_text_pos, close_galley, close_color);
        }

        // -- Interaction handling --
        if response.clicked() {
            if pointer_in_close && (is_active || is_hovered) {
                *tab_to_close = Some(idx);
            } else {
                self.tabs.switch_to(idx);
            }
        }

        if response.middle_clicked() {
            *tab_to_close = Some(idx);
        }

        // -- 1px separator between tabs --
        if idx < self.tabs.tab_count() - 1 {
            painter.line_segment(
                [
                    egui::Pos2::new(tab_rect.max.x, tab_rect.min.y + 4.0),
                    egui::Pos2::new(tab_rect.max.x, tab_rect.max.y - 4.0),
                ],
                Stroke::new(1.0, visuals.widgets.noninteractive.bg_stroke.color),
            );
        }

        self.render_tab_context_menu(ui, idx, &response, tab_to_close);
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
