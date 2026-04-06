//! Tab bar rendering for the editor application.
//!
//! Handles the tab strip with active tab highlighting, close buttons,
//! context menus, middle-click close, new tab creation, and horizontal
//! scrolling when tabs overflow the available width.

use eframe::egui;
use egui::{Color32, Rect, RichText, ScrollArea, Sense, Stroke, Vec2, Visuals};

use super::App;

/// Horizontal padding on each side of the tab content.
const TAB_PADDING: f32 = 8.0;
/// Gap between the title text and the close button area.
const TITLE_CLOSE_GAP: f32 = 4.0;
/// Side length of the square close button area.
const CLOSE_AREA_SIZE: f32 = 14.0;
/// Fixed tab height.
const TAB_HEIGHT: f32 = 32.0;
/// Pixels to scroll per arrow button click.
const SCROLL_STEP: f32 = 120.0;
/// Width of each scroll arrow button.
const ARROW_BUTTON_WIDTH: f32 = 20.0;

impl App {
    /// Renders the tab bar with active tab highlighting, close buttons,
    /// and horizontal scrolling when tabs overflow.
    pub(crate) fn show_tab_bar(&mut self, ui: &mut egui::Ui) {
        let visuals = ui.visuals().clone();

        // Detect whether the active tab or tab count changed since last frame.
        let active_changed = self.tabs.active != self.prev_active_tab;
        let count_changed = self.tabs.tab_count() != self.prev_tab_count;
        let need_auto_scroll = active_changed || count_changed;

        // Update tracked state for next frame.
        self.prev_active_tab = self.tabs.active;
        self.prev_tab_count = self.tabs.tab_count();

        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            let mut tab_to_close: Option<usize> = None;

            // Collect tab rects for auto-scroll calculation.
            let mut tab_rects: Vec<Rect> = Vec::with_capacity(self.tabs.tab_count());

            // Reserve space for elements that render after the ScrollArea.
            // Uses previous frame's overflow flag (one-frame lag is acceptable).
            let arrows_width = if self.tabs_overflow {
                ARROW_BUTTON_WIDTH * 2.0
            } else {
                0.0
            };
            let new_tab_btn_width = 24.0;
            let reserved = arrows_width + new_tab_btn_width;
            let scroll_max_width = (ui.available_width() - reserved).max(0.0);

            // 1. Render scrollable tab area.
            // Enable vertical-wheel → horizontal-scroll mapping so the user
            // can scroll tabs with a normal mouse wheel.
            ui.style_mut().always_scroll_the_only_direction = true;
            let scroll_output = ScrollArea::horizontal()
                .id_salt("tab_scroll")
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                .horizontal_scroll_offset(self.tab_scroll_offset)
                .max_width(scroll_max_width)
                .show(ui, |ui: &mut egui::Ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    for idx in 0..self.tabs.tab_count() {
                        let rect = self.render_tab_button(ui, idx, &visuals, &mut tab_to_close);
                        tab_rects.push(rect);
                    }
                });

            // 2. Update scroll state from ScrollArea output.
            self.tab_scroll_offset = scroll_output.state.offset.x;
            self.tabs_overflow = scroll_output.content_size.x > scroll_output.inner_rect.width();

            // 3. Auto-scroll to the active tab if it changed.
            if need_auto_scroll {
                if let Some(active_rect) = tab_rects.get(self.tabs.active).copied() {
                    self.auto_scroll_to_tab(active_rect, &scroll_output);
                }
            }

            // 4. Render scroll arrows (only when overflow).
            if self.tabs_overflow {
                let max_offset =
                    (scroll_output.content_size.x - scroll_output.inner_rect.width()).max(0.0);
                self.render_scroll_arrows(ui, &visuals, max_offset);
            }

            // 5. "+" button and empty area (unchanged).
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
    /// Returns the allocated tab rect for auto-scroll calculations.
    fn render_tab_button(
        &mut self,
        ui: &mut egui::Ui,
        idx: usize,
        visuals: &Visuals,
        tab_to_close: &mut Option<usize>,
    ) -> Rect {
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

        tab_rect
    }

    /// Adjusts the scroll offset so that `target_rect` (in scroll content
    /// coordinates) is fully visible within the scroll area.
    fn auto_scroll_to_tab(
        &mut self,
        target_rect: Rect,
        scroll_output: &egui::scroll_area::ScrollAreaOutput<()>,
    ) {
        let visible_min = scroll_output.state.offset.x;
        let visible_width = scroll_output.inner_rect.width();
        let visible_max = visible_min + visible_width;

        // Convert the tab rect from screen coordinates to scroll content coordinates
        // by subtracting the inner_rect origin and adding back the offset.
        let content_left = target_rect.min.x - scroll_output.inner_rect.min.x + visible_min;
        let content_right = target_rect.max.x - scroll_output.inner_rect.min.x + visible_min;

        let padding = TAB_PADDING;

        if content_left < visible_min {
            // Tab is to the left of the visible area — scroll left.
            self.tab_scroll_offset = (content_left - padding).max(0.0);
        } else if content_right > visible_max {
            // Tab is to the right of the visible area — scroll right.
            let max_offset = (scroll_output.content_size.x - visible_width).max(0.0);
            self.tab_scroll_offset = (content_right - visible_width + padding).min(max_offset);
        }
        // Otherwise the tab is already fully visible — no adjustment needed.
    }

    /// Renders the left/right scroll arrow buttons.
    fn render_scroll_arrows(&mut self, ui: &mut egui::Ui, visuals: &Visuals, max_offset: f32) {
        ui.spacing_mut().item_spacing.x = 0.0;

        let at_start = self.tab_scroll_offset <= 0.0;
        let at_end = self.tab_scroll_offset >= max_offset;

        // Left arrow
        let left_color = if at_start {
            visuals
                .widgets
                .noninteractive
                .fg_stroke
                .color
                .gamma_multiply(0.3)
        } else {
            visuals.widgets.noninteractive.fg_stroke.color
        };
        let left_btn = egui::Button::new(RichText::new("\u{25C0}").color(left_color).size(10.0))
            .fill(Color32::TRANSPARENT)
            .stroke(Stroke::NONE)
            .min_size(Vec2::new(ARROW_BUTTON_WIDTH, TAB_HEIGHT));

        if ui.add(left_btn).clicked() && !at_start {
            self.tab_scroll_offset = (self.tab_scroll_offset - SCROLL_STEP).max(0.0);
        }

        // Right arrow
        let right_color = if at_end {
            visuals
                .widgets
                .noninteractive
                .fg_stroke
                .color
                .gamma_multiply(0.3)
        } else {
            visuals.widgets.noninteractive.fg_stroke.color
        };
        let right_btn = egui::Button::new(RichText::new("\u{25B6}").color(right_color).size(10.0))
            .fill(Color32::TRANSPARENT)
            .stroke(Stroke::NONE)
            .min_size(Vec2::new(ARROW_BUTTON_WIDTH, TAB_HEIGHT));

        if ui.add(right_btn).clicked() && !at_end {
            self.tab_scroll_offset = (self.tab_scroll_offset + SCROLL_STEP).min(max_offset);
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
