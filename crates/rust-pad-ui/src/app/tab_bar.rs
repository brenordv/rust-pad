//! Tab bar rendering for the editor application.
//!
//! Handles the tab strip with active tab highlighting, close buttons,
//! context menus, middle-click close, new tab creation, and horizontal
//! scrolling when tabs overflow the available width.

use eframe::egui;
use egui::{Color32, Rect, RichText, ScrollArea, Sense, Stroke, Vec2, Visuals};

use super::App;

/// Transient state tracked while the user is dragging a tab to reorder it.
///
/// Created on `drag_started`, updated every frame while the pointer moves,
/// and cleared on drag stop (commit) or cancel (Escape). The visual cue is
/// "dim the source tab in place + paint an insertion indicator at the drop
/// target", so the struct only needs to remember which tab is being dragged
/// and where it would be dropped on release.
#[derive(Debug, Clone, Copy)]
pub(crate) struct TabDragState {
    /// Current index of the tab being dragged.
    pub source_idx: usize,
    /// Insertion position where the tab would be dropped on release.
    /// This is the index the dragged tab will occupy after the move.
    pub insert_idx: usize,
}

/// Deferred tab bar action to execute after the rendering loop completes.
///
/// Context menu actions that modify the tab list cannot run during the
/// rendering loop because the loop iterates over tab indices that would
/// become stale. These are collected and executed afterwards.
enum DeferredTabAction {
    /// Close all tabs except the one at the given index.
    Others(usize),
    /// Close all unchanged tabs.
    Unchanged,
    /// Close all tabs.
    All,
    /// Pin the tab at the given index.
    Pin(usize),
    /// Unpin the tab at the given index.
    Unpin(usize),
    /// Set the tab color (or clear it when `None`) on the tab at the given index.
    SetTabColor(usize, Option<rust_pad_core::tab_color::TabColor>),
}

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
/// Width of the vertical insertion indicator drawn between tabs while dragging.
const DRAG_INDICATOR_WIDTH: f32 = 3.0;

/// Returns the `[start, end)` index range for the drag section that contains
/// `source_idx`.
///
/// Pinned tabs occupy `0..pinned_count`; unpinned tabs occupy
/// `pinned_count..tab_count`. Drag-and-drop reorders are clamped to the
/// source tab's own section so pinned and unpinned tabs cannot cross the
/// boundary.
fn drag_section(source_idx: usize, pinned_count: usize, tab_count: usize) -> (usize, usize) {
    if source_idx < pinned_count {
        (0, pinned_count)
    } else {
        (pinned_count, tab_count)
    }
}

/// Computes the target `insert_idx` for a tab drag, given the pointer's
/// current x coordinate.
///
/// The result is the argument that should be passed to
/// [`TabManager::move_tab`]: it is the index the dragged tab will occupy in
/// the vector *after* the move, counted as if the source tab had already
/// been removed. The result is clamped to `[section_start, section_end - 1]`.
///
/// The algorithm counts how many non-source tabs in the source's section
/// have their centers to the left of the pointer; that count, offset by
/// `section_start`, is the insert index.
fn compute_insert_idx(
    tab_rects: &[Rect],
    source_idx: usize,
    section_start: usize,
    section_end: usize,
    pointer_x: f32,
) -> usize {
    let mut left_count = 0usize;
    for (i, rect) in tab_rects
        .iter()
        .enumerate()
        .take(section_end)
        .skip(section_start)
    {
        if i == source_idx {
            continue;
        }
        if rect.center().x < pointer_x {
            left_count += 1;
        }
    }
    section_start + left_count
}

/// Computes the screen-x position where the vertical insertion indicator
/// should be drawn for the given `insert_idx`.
///
/// The indicator sits at the gap between two tabs in the *post-removal*
/// list. This function translates that back to the current (source-still-
/// in-place) `tab_rects` layout so the line can be drawn without mutating
/// the tab vector.
///
/// Falls back to the source tab's left edge when the section contains only
/// the source tab; callers typically skip drawing the indicator entirely in
/// that case.
fn compute_drag_indicator_x(
    tab_rects: &[Rect],
    source_idx: usize,
    section_start: usize,
    section_end: usize,
    insert_idx: usize,
) -> f32 {
    // Walk the non-source tabs in order; the `left_count`-th gap lies
    // immediately to the left of the (left_count)-th non-source tab.
    let left_count = insert_idx.saturating_sub(section_start);
    let mut seen = 0usize;
    for (i, rect) in tab_rects
        .iter()
        .enumerate()
        .take(section_end)
        .skip(section_start)
    {
        if i == source_idx {
            continue;
        }
        if seen == left_count {
            return rect.min.x;
        }
        seen += 1;
    }
    // Pointer is past every non-source tab — draw at the right edge of the
    // last non-source tab in the section.
    for (i, rect) in tab_rects
        .iter()
        .enumerate()
        .take(section_end)
        .skip(section_start)
        .rev()
    {
        if i != source_idx {
            return rect.max.x;
        }
    }
    // Fallback: the section contains only the source tab.
    tab_rects
        .get(source_idx)
        .map(|r| r.min.x)
        .unwrap_or_default()
}

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

        // Handle Escape cancellation before the render loop so the drag state
        // is cleared before we read it for visual feedback. Vertical pointer
        // departure is deliberately NOT treated as cancel — see the Phase 3
        // spec (accessibility: users who cannot hold a straight horizontal
        // line must not lose an in-progress drag).
        if self.tab_drag.is_some() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.tab_drag = None;
        }

        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            let mut tab_to_close: Option<usize> = None;
            let mut deferred_action: Option<DeferredTabAction> = None;
            let mut drag_commit: Option<(usize, usize)> = None;

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
                        let rect = self.render_tab_button(
                            ui,
                            idx,
                            &visuals,
                            &mut tab_to_close,
                            &mut deferred_action,
                        );
                        tab_rects.push(rect);
                    }

                    // Process in-progress drag: update insert_idx, paint the
                    // insertion indicator, and detect drop. Done inside the
                    // ScrollArea closure so the indicator is drawn in the
                    // same coordinate space as the tabs and is naturally
                    // clipped to the visible tab region.
                    self.process_tab_drag(ui, &tab_rects, &mut drag_commit);
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

            // 6. Commit any completed drag. The drop was detected inside
            // process_tab_drag, but the actual move happens here to keep
            // mutations of `self.tabs.documents` out of the render loop.
            if let Some((from, to)) = drag_commit {
                self.tabs.move_tab(from, to);
                self.tab_drag = None;
            }

            // 7. Execute deferred actions after the rendering loop.
            if let Some(idx) = tab_to_close {
                self.request_close_tab(idx);
            }
            if let Some(action) = deferred_action {
                match action {
                    DeferredTabAction::Others(keep_idx) => {
                        self.tabs.switch_to(keep_idx);
                        self.close_all_but_active();
                    }
                    DeferredTabAction::Unchanged => {
                        self.close_unchanged_tabs();
                    }
                    DeferredTabAction::All => {
                        self.close_all_tabs();
                    }
                    DeferredTabAction::Pin(idx) => {
                        self.tabs.pin_tab(idx);
                    }
                    DeferredTabAction::Unpin(idx) => {
                        self.tabs.unpin_tab(idx);
                    }
                    DeferredTabAction::SetTabColor(idx, color) => {
                        if idx < self.tabs.documents.len() {
                            self.tabs.documents[idx].tab_color = color;
                        }
                    }
                }
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
        deferred_action: &mut Option<DeferredTabAction>,
    ) -> Rect {
        let doc = &self.tabs.documents[idx];
        let is_active = idx == self.tabs.active;
        let tab_color = doc.tab_color;
        let is_drag_source = self.tab_drag.is_some_and(|d| d.source_idx == idx);

        // Title composition: optional pin glyph + title + optional modified marker.
        // The pushpin emoji (U+1F4CC) renders via NotoEmoji-Regular.ttf which egui
        // ships in its default font set, so no font setup is required.
        let title = match (doc.pinned, doc.modified) {
            (true, true) => format!("\u{1F4CC} {} *", doc.title),
            (true, false) => format!("\u{1F4CC} {}", doc.title),
            (false, true) => format!("{} *", doc.title),
            (false, false) => doc.title.clone(),
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
        // Dim the dragged tab so the user sees where it came from while the
        // drop position is indicated separately by the insertion line.
        let fill = if is_drag_source {
            fill.gamma_multiply(0.45)
        } else {
            fill
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

        // -- Paint accent line --
        // Priority: user-assigned tab color > active theme accent.
        // A tab with a custom color always shows its accent, even when
        // inactive. An active tab without a custom color falls back to
        // the theme accent (existing behavior).
        let accent_stroke_color = match tab_color {
            Some(c) => {
                let [r, g, b] = c.to_rgb();
                Some(Color32::from_rgb(r, g, b))
            }
            None if is_active => Some(self.theme_ctrl.accent_color),
            None => None,
        };
        if let Some(color) = accent_stroke_color {
            painter.line_segment(
                [
                    egui::Pos2::new(tab_rect.min.x, tab_rect.min.y),
                    egui::Pos2::new(tab_rect.max.x, tab_rect.min.y),
                ],
                Stroke::new(2.0, color),
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
        // `clicked()` does NOT fire if the widget was dragged, so the click
        // and drag handlers are naturally mutually exclusive.
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

        // -- Drag start detection --
        // Ignore drags that originated on the close button: the user is
        // about to click close, not reorder. We check `press_origin` (the
        // point the button was first pressed) rather than the current
        // hover position so a drag that *started* over the × but has since
        // moved is still excluded from starting a reorder.
        if response.drag_started() {
            let press_in_close = ui
                .input(|i| i.pointer.press_origin())
                .is_some_and(|p| close_rect.contains(p));
            if !press_in_close {
                self.tab_drag = Some(TabDragState {
                    source_idx: idx,
                    insert_idx: idx,
                });
                // Also make sure the dragged tab becomes active so the
                // editor reflects what the user is manipulating.
                self.tabs.switch_to(idx);
            }
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

        self.render_tab_context_menu(ui, idx, &response, tab_to_close, deferred_action);

        tab_rect
    }

    /// Processes an in-progress tab drag: updates the target insert index,
    /// paints the vertical insertion indicator, and detects drop.
    ///
    /// Called once per frame inside the ScrollArea closure, after all tab
    /// buttons have been rendered, so it has the full set of `tab_rects` to
    /// hit-test against. The commit is deferred through `drag_commit` so the
    /// caller performs the actual `move_tab` outside the render loop.
    ///
    /// The pointer may leave the tab bar vertically without cancelling the
    /// drag (accessibility: users who cannot hold a perfectly horizontal
    /// line must not lose an in-progress reorder).
    fn process_tab_drag(
        &mut self,
        ui: &mut egui::Ui,
        tab_rects: &[Rect],
        drag_commit: &mut Option<(usize, usize)>,
    ) {
        let Some(drag) = self.tab_drag else {
            return;
        };

        // Guard: if the document vector shrank underneath us (e.g. via some
        // other action this frame), abort the drag cleanly.
        if drag.source_idx >= tab_rects.len() {
            self.tab_drag = None;
            return;
        }

        // Clamp the insert range to the section the source tab belongs to,
        // so pinned tabs cannot cross into the unpinned area and vice versa.
        let pinned_count = self.tabs.pinned_count();
        let (section_start, section_end) =
            drag_section(drag.source_idx, pinned_count, tab_rects.len());

        // Read the latest pointer position. `latest_pos` survives the
        // pointer leaving any widget, which is exactly what we need for the
        // "don't cancel on vertical departure" requirement.
        let pointer_x = ui
            .input(|i| i.pointer.latest_pos())
            .map(|p| p.x)
            .unwrap_or(tab_rects[drag.source_idx].center().x);

        let new_insert_idx = compute_insert_idx(
            tab_rects,
            drag.source_idx,
            section_start,
            section_end,
            pointer_x,
        );

        // Persist the latest insert target so the next frame still knows
        // where the drop would land if the pointer stops moving.
        if let Some(state) = self.tab_drag.as_mut() {
            state.insert_idx = new_insert_idx;
        }

        // Paint the insertion indicator. When the section is effectively
        // empty (only the source tab), there is nowhere to drop, so skip
        // the indicator entirely.
        let section_len_excluding_source =
            section_end.saturating_sub(section_start).saturating_sub(1);
        if section_len_excluding_source > 0 {
            let indicator_x = compute_drag_indicator_x(
                tab_rects,
                drag.source_idx,
                section_start,
                section_end,
                new_insert_idx,
            );
            let indicator_y_min = tab_rects[drag.source_idx].min.y;
            let indicator_y_max = tab_rects[drag.source_idx].max.y;
            let accent = self.theme_ctrl.accent_color;
            ui.painter().line_segment(
                [
                    egui::Pos2::new(indicator_x, indicator_y_min),
                    egui::Pos2::new(indicator_x, indicator_y_max),
                ],
                Stroke::new(DRAG_INDICATOR_WIDTH, accent),
            );
        }

        // Drop detection. Using the global pointer state (rather than the
        // source tab's Response) means the drop fires even if the pointer
        // has left the tab bar entirely, which matches the accessibility
        // requirement.
        let released = ui.input(|i| i.pointer.any_released() && !i.pointer.primary_down());
        if released {
            if new_insert_idx != drag.source_idx {
                *drag_commit = Some((drag.source_idx, new_insert_idx));
            } else {
                // No movement → nothing to commit, but we still need to
                // clear the drag state. The caller only clears when
                // drag_commit is Some, so do it here for the no-op case.
                self.tab_drag = None;
            }
        }
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
    ///
    /// Bulk-close actions are deferred to avoid mutating the tab list while
    /// the rendering loop is still iterating over tab indices.
    fn render_tab_context_menu(
        &mut self,
        _ui: &mut egui::Ui,
        idx: usize,
        response: &egui::Response,
        tab_to_close: &mut Option<usize>,
        deferred_action: &mut Option<DeferredTabAction>,
    ) {
        let is_pinned = self.tabs.documents[idx].pinned;
        response.context_menu(|ui| {
            if ui.button("Close").clicked() {
                *tab_to_close = Some(idx);
                ui.close();
            }
            if ui.button("Close Others").clicked() {
                *deferred_action = Some(DeferredTabAction::Others(idx));
                ui.close();
            }
            if ui.button("Close Unchanged").clicked() {
                *deferred_action = Some(DeferredTabAction::Unchanged);
                ui.close();
            }
            if ui.button("Close All").clicked() {
                *deferred_action = Some(DeferredTabAction::All);
                ui.close();
            }
            ui.separator();
            let pin_label = if is_pinned { "Unpin Tab" } else { "Pin Tab" };
            if ui.button(pin_label).clicked() {
                *deferred_action = Some(if is_pinned {
                    DeferredTabAction::Unpin(idx)
                } else {
                    DeferredTabAction::Pin(idx)
                });
                ui.close();
            }
            ui.menu_button("Set Tab Color", |ui| {
                for variant in rust_pad_core::tab_color::TabColor::ALL {
                    let [r, g, b] = variant.to_rgb();
                    let label =
                        egui::RichText::new(variant.label()).color(Color32::from_rgb(r, g, b));
                    if ui.button(label).clicked() {
                        *deferred_action = Some(DeferredTabAction::SetTabColor(idx, Some(variant)));
                        ui.close();
                    }
                }
                ui.separator();
                if ui.button("Clear Color").clicked() {
                    *deferred_action = Some(DeferredTabAction::SetTabColor(idx, None));
                    ui.close();
                }
            });
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

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Pos2, Rect};

    /// Builds `count` non-overlapping tab rects of width 100 at y=0..32.
    /// Tab `i` occupies x in `[i * 100, i * 100 + 100)`, so its center is at
    /// `i * 100 + 50`.
    fn rects(count: usize) -> Vec<Rect> {
        (0..count)
            .map(|i| {
                Rect::from_min_max(
                    Pos2::new(i as f32 * 100.0, 0.0),
                    Pos2::new(i as f32 * 100.0 + 100.0, 32.0),
                )
            })
            .collect()
    }

    // ── drag_section ────────────────────────────────────────────────

    #[test]
    fn drag_section_no_pinned_covers_full_range() {
        // 4 tabs, none pinned → section is the whole bar for any source.
        assert_eq!(drag_section(0, 0, 4), (0, 4));
        assert_eq!(drag_section(3, 0, 4), (0, 4));
    }

    #[test]
    fn drag_section_all_pinned_covers_full_range() {
        // 4 tabs, all pinned → section is the whole bar for any source.
        assert_eq!(drag_section(0, 4, 4), (0, 4));
        assert_eq!(drag_section(3, 4, 4), (0, 4));
    }

    #[test]
    fn drag_section_pinned_source_uses_pinned_section() {
        // 5 tabs, 2 pinned. Source in pinned section → range = [0, 2).
        assert_eq!(drag_section(0, 2, 5), (0, 2));
        assert_eq!(drag_section(1, 2, 5), (0, 2));
    }

    #[test]
    fn drag_section_unpinned_source_uses_unpinned_section() {
        // 5 tabs, 2 pinned. Source in unpinned section → range = [2, 5).
        assert_eq!(drag_section(2, 2, 5), (2, 5));
        assert_eq!(drag_section(4, 2, 5), (2, 5));
    }

    // ── compute_insert_idx ──────────────────────────────────────────

    #[test]
    fn insert_idx_left_of_everything_returns_section_start() {
        let r = rects(4);
        // Pointer far to the left of tab 0; source = 2.
        // Section = [0, 4). No non-source tabs with center < pointer.
        assert_eq!(compute_insert_idx(&r, 2, 0, 4, -50.0), 0);
    }

    #[test]
    fn insert_idx_right_of_everything_returns_section_end_minus_one() {
        let r = rects(4);
        // Pointer far to the right of tab 3; source = 0.
        // After removing source, 3 non-source tabs remain; all have center
        // < pointer → left_count = 3 → insert_idx = section_start + 3 = 3.
        assert_eq!(compute_insert_idx(&r, 0, 0, 4, 9999.0), 3);
    }

    #[test]
    fn insert_idx_pointer_between_neighbors_picks_left_count() {
        let r = rects(4);
        // Source = 0 (removed from count). Tabs 1,2,3 have centers 150,250,350.
        // Pointer at 200: tab 1 center (150) < 200, tab 2 (250) ≥ 200, tab 3 same.
        // left_count = 1 → insert_idx = 1.
        assert_eq!(compute_insert_idx(&r, 0, 0, 4, 200.0), 1);
    }

    #[test]
    fn insert_idx_source_exclusion_prevents_self_count() {
        let r = rects(4);
        // Source = 2 (center 250). Pointer at 260 → source's center (250)
        // would count if we didn't exclude it. Non-source centers:
        // tab0=50, tab1=150, tab3=350. Only tabs 0 and 1 have center < 260
        // → left_count = 2 → insert_idx = 2.
        assert_eq!(compute_insert_idx(&r, 2, 0, 4, 260.0), 2);
    }

    #[test]
    fn insert_idx_clamped_to_pinned_section() {
        let r = rects(5);
        // 5 tabs, pinned_count = 2, source = 0 (pinned). Section = [0, 2).
        // Pointer far right — should still clamp inside pinned section.
        // Non-source in section: only tab 1. left_count = 1 → insert_idx = 1.
        assert_eq!(compute_insert_idx(&r, 0, 0, 2, 9999.0), 1);
    }

    #[test]
    fn insert_idx_clamped_to_unpinned_section() {
        let r = rects(5);
        // 5 tabs, pinned_count = 2, source = 4 (unpinned, last).
        // Section = [2, 5). Pointer far left → left_count = 0 → insert_idx = 2.
        assert_eq!(compute_insert_idx(&r, 4, 2, 5, -50.0), 2);
    }

    #[test]
    fn insert_idx_same_position_when_pointer_matches_source_center() {
        let r = rects(4);
        // Source = 1 (center 150). Pointer at 150 → only tab 0 (center 50)
        // has center < 150 → left_count = 1 → insert_idx = 1 (same slot).
        assert_eq!(compute_insert_idx(&r, 1, 0, 4, 150.0), 1);
    }

    // ── compute_drag_indicator_x ────────────────────────────────────

    #[test]
    fn indicator_x_at_section_start_is_left_edge_of_first_non_source() {
        let r = rects(4);
        // Source = 2; insert_idx = 0 → indicator at left edge of tab 0.
        assert_eq!(compute_drag_indicator_x(&r, 2, 0, 4, 0), r[0].min.x);
    }

    #[test]
    fn indicator_x_when_source_is_first_skips_to_second_tab() {
        let r = rects(4);
        // Source = 0; insert_idx = 0 → first non-source in [0, 4) is tab 1
        // → indicator at left edge of tab 1.
        assert_eq!(compute_drag_indicator_x(&r, 0, 0, 4, 0), r[1].min.x);
    }

    #[test]
    fn indicator_x_between_middle_tabs() {
        let r = rects(4);
        // Source = 0, insert_idx = 2. Non-source walk: tab 1 (seen 0), tab 2
        // (seen 1), tab 3 (seen 2 == left_count=2) → returns tab 3's left edge.
        assert_eq!(compute_drag_indicator_x(&r, 0, 0, 4, 2), r[3].min.x);
    }

    #[test]
    fn indicator_x_past_everything_returns_last_non_source_right_edge() {
        let r = rects(4);
        // Source = 0, insert_idx = 3 (section end). Walk finds no tab with
        // seen == 3, falls through to reverse scan → last non-source is
        // tab 3 → returns its right edge.
        assert_eq!(compute_drag_indicator_x(&r, 0, 0, 4, 3), r[3].max.x);
    }

    #[test]
    fn indicator_x_past_everything_when_source_is_last() {
        let r = rects(4);
        // Source = 3 (last), insert_idx = 3 (end of section [0,4)). Walk:
        // tab 0 (seen 0), tab 1 (seen 1), tab 2 (seen 2), tab 3 skipped.
        // No match at seen == 3 → reverse scan returns tab 2's right edge.
        assert_eq!(compute_drag_indicator_x(&r, 3, 0, 4, 3), r[2].max.x);
    }

    #[test]
    fn indicator_x_within_pinned_section_only() {
        let r = rects(5);
        // 5 tabs, pinned_count = 2, source = 0, insert_idx = 1.
        // Section [0, 2). Non-source walk: tab 1 (seen 0 == left_count=1? no,
        // left_count = 1 - 0 = 1, so seen must equal 1). tab 1 has seen = 0,
        // no match; reverse scan returns tab 1's right edge.
        assert_eq!(compute_drag_indicator_x(&r, 0, 0, 2, 1), r[1].max.x);
    }

    #[test]
    fn indicator_x_unpinned_section_ignores_pinned_tabs() {
        let r = rects(5);
        // 5 tabs, source = 2 (first unpinned), section [2, 5), insert_idx = 2.
        // left_count = 0. First non-source tab in section is tab 3 (seen 0 ==
        // 0) → returns tab 3's left edge. Pinned tabs 0/1 are never touched.
        assert_eq!(compute_drag_indicator_x(&r, 2, 2, 5, 2), r[3].min.x);
    }

    #[test]
    fn indicator_x_single_tab_section_falls_back_to_source_left_edge() {
        let r = rects(3);
        // Section [0, 1) with source = 0 — the source is the only tab in
        // the section. No non-source tabs found; fallback returns source
        // left edge. (In practice process_tab_drag skips drawing in this
        // case, but the function should still be safe.)
        assert_eq!(compute_drag_indicator_x(&r, 0, 0, 1, 0), r[0].min.x);
    }

    #[test]
    fn indicator_x_empty_rects_fallback_returns_zero() {
        let r: Vec<Rect> = Vec::new();
        // No rects at all; fallback returns Default (0.0). Guards against
        // out-of-bounds if the caller ever invokes this with an empty slice.
        assert_eq!(compute_drag_indicator_x(&r, 0, 0, 0, 0), 0.0);
    }

    // ── TabDragState construction ──────────────────────────────────

    #[test]
    fn tab_drag_state_is_copy_and_preserves_fields() {
        let s = TabDragState {
            source_idx: 3,
            insert_idx: 7,
        };
        let c = s; // Copy
        assert_eq!(c.source_idx, 3);
        assert_eq!(c.insert_idx, 7);
    }
}
