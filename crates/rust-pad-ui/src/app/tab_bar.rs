//! Tab bar rendering for the editor application.
//!
//! Handles the tab strip with active tab highlighting, close buttons,
//! context menus, middle-click close, new tab creation, and horizontal
//! scrolling when tabs overflow the available width.

use std::path::Path;
use std::sync::Arc;

use eframe::egui;
use egui::{Color32, Galley, Rect, RichText, ScrollArea, Sense, Stroke, Vec2, Visuals};

use super::App;
use crate::app::workspace_ops::copy_path_root_for;
use crate::tabs::PaneId;
use crate::workspace::menus::{copy_path_menu, CopyPathRequest};
use rust_pad_core::document::Document;
use rust_pad_core::tab_color::TabColor;

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
    /// Copy a representation of the tab's file path to the clipboard.
    CopyPath(CopyPathRequest),
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

/// Formats a tab title with optional pin glyph and modified marker.
///
/// The pushpin emoji (U+1F4CC) renders via the `NotoEmoji-Regular.ttf`
/// that egui ships in its default font set, so no font setup is required.
fn format_tab_title(pinned: bool, modified: bool, title: &str) -> String {
    match (pinned, modified) {
        (true, true) => format!("\u{1F4CC} {title} *"),
        (true, false) => format!("\u{1F4CC} {title}"),
        (false, true) => format!("{title} *"),
        (false, false) => title.to_string(),
    }
}

/// Picks the title text color for a tab based on its active state.
///
/// Active tabs use a high-contrast color tuned for the current theme;
/// inactive tabs fall back to the noninteractive widget foreground.
fn title_color_for(is_active: bool, visuals: &Visuals) -> Color32 {
    if !is_active {
        return visuals.widgets.noninteractive.fg_stroke.color;
    }
    if visuals.dark_mode {
        Color32::from_rgb(220, 220, 220)
    } else {
        Color32::from_rgb(30, 30, 30)
    }
}

/// Resolves the accent stroke color shown above a tab.
///
/// Priority: an explicit per-tab color always wins; otherwise the active
/// tab gets the theme accent and inactive tabs have no accent line.
fn resolve_accent_color(
    tab_color: Option<TabColor>,
    is_active: bool,
    active_accent: Color32,
) -> Option<Color32> {
    match tab_color {
        Some(c) => {
            let [r, g, b] = c.to_rgb();
            Some(Color32::from_rgb(r, g, b))
        }
        None if is_active => Some(active_accent),
        None => None,
    }
}

/// Computes the full tab width given the laid-out title width.
fn compute_tab_width(title_width: f32) -> f32 {
    TAB_PADDING + title_width + TITLE_CLOSE_GAP + CLOSE_AREA_SIZE + TAB_PADDING
}

/// Inputs to [`paint_tab_chrome`].
///
/// Grouped into a struct because the helper would otherwise take ten
/// positional arguments and trip clippy's `too_many_arguments` lint.
struct TabChrome {
    tab_rect: Rect,
    title_galley: Arc<Galley>,
    title_color: Color32,
    is_active: bool,
    is_hovered: bool,
    is_drag_source: bool,
    accent: Option<Color32>,
}

/// Renders the visual chrome for a single tab button: background, accent
/// line, title text, and the close-button glyph (when active or hovered).
///
/// Returns the close-button rect and whether the pointer is currently
/// over it, so the caller can wire up its own click handling. The same
/// helper drives both the single-pane tab bar (`render_tab_button`) and
/// the per-pane tab bar (`render_pane_tab_button`).
fn paint_tab_chrome(ui: &egui::Ui, visuals: &Visuals, chrome: &TabChrome) -> (Rect, bool) {
    let painter = ui.painter().clone();
    let TabChrome {
        tab_rect,
        title_galley,
        title_color,
        is_active,
        is_hovered,
        is_drag_source,
        accent,
    } = chrome;
    let tab_rect = *tab_rect;
    let is_active = *is_active;
    let is_hovered = *is_hovered;

    // -- Background --
    let mut fill = if is_active {
        visuals.widgets.active.bg_fill
    } else if is_hovered {
        visuals.widgets.hovered.weak_bg_fill
    } else {
        visuals.faint_bg_color
    };
    if *is_drag_source {
        // Dim the dragged tab so the user sees where it came from while
        // the drop position is indicated separately by the insertion line.
        fill = fill.gamma_multiply(0.45);
    }
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

    // -- Accent line --
    if let Some(color) = accent {
        painter.line_segment(
            [
                egui::Pos2::new(tab_rect.min.x, tab_rect.min.y),
                egui::Pos2::new(tab_rect.max.x, tab_rect.min.y),
            ],
            Stroke::new(2.0, *color),
        );
    }

    // -- Title text (vertically centered, after left padding) --
    let title_pos = egui::Pos2::new(
        tab_rect.min.x + TAB_PADDING,
        tab_rect.center().y - title_galley.size().y / 2.0,
    );
    painter.galley(title_pos, title_galley.clone(), *title_color);

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

    if is_active || is_hovered {
        if pointer_in_close {
            painter.rect_filled(close_rect, 2.0, visuals.widgets.hovered.bg_fill);
        }
        let close_font = egui::FontId::proportional(14.0);
        let close_color = visuals.widgets.noninteractive.fg_stroke.color;
        let close_galley = painter.layout_no_wrap("\u{00D7}".to_owned(), close_font, close_color);
        let close_text_pos = egui::Pos2::new(
            close_rect.center().x - close_galley.size().x / 2.0,
            close_rect.center().y - close_galley.size().y / 2.0,
        );
        painter.galley(close_text_pos, close_galley, close_color);
    }

    (close_rect, pointer_in_close)
}

/// Lays out the tab title galley and computes its display color and
/// the full tab size.
///
/// Both the single-pane and per-pane tab bars share this exact prelude;
/// extracting it keeps tab rendering pixel-identical between the two.
fn layout_tab_title(
    ui: &egui::Ui,
    visuals: &Visuals,
    doc: &Document,
    is_active: bool,
) -> (Arc<Galley>, Color32, Vec2) {
    let title = format_tab_title(doc.pinned, doc.modified, &doc.title);
    let title_color = title_color_for(is_active, visuals);
    let title_font = egui::FontId::proportional(14.0);
    let title_galley = ui.painter().layout_no_wrap(title, title_font, title_color);
    let tab_width = compute_tab_width(title_galley.size().x);
    let tab_size = Vec2::new(tab_width, TAB_HEIGHT);
    (title_galley, title_color, tab_size)
}

/// Computes the scroll offset needed to make `target_rect` fully visible.
///
/// Converts the tab rect from screen coordinates to scroll content
/// coordinates and adjusts the offset so the tab is within the visible
/// region. Returns the current offset unchanged when the tab is already
/// fully visible.
///
/// Parameters:
/// - `target_rect`: screen-space rect of the tab to scroll into view.
/// - `inner_rect`: the scroll area's visible viewport rect.
/// - `content_width`: total width of the scrollable content.
/// - `current_offset`: current horizontal scroll offset.
fn auto_scroll_offset(
    target_rect: Rect,
    inner_rect: Rect,
    content_width: f32,
    current_offset: f32,
) -> f32 {
    let visible_min = current_offset;
    let visible_width = inner_rect.width();
    let visible_max = visible_min + visible_width;

    let content_left = target_rect.min.x - inner_rect.min.x + visible_min;
    let content_right = target_rect.max.x - inner_rect.min.x + visible_min;

    let padding = TAB_PADDING;

    if content_left < visible_min {
        (content_left - padding).max(0.0)
    } else if content_right > visible_max {
        let max_offset = (content_width - visible_width).max(0.0);
        (content_right - visible_width + padding).min(max_offset)
    } else {
        current_offset
    }
}

/// Renders a pair of left/right scroll arrow buttons.
///
/// Used by both the main tab bar and per-pane tab bars. Updates `offset`
/// in place when an arrow is clicked.
fn render_scroll_arrow_pair(
    ui: &mut egui::Ui,
    visuals: &Visuals,
    offset: &mut f32,
    max_offset: f32,
) {
    ui.spacing_mut().item_spacing.x = 0.0;

    let at_start = *offset <= 0.0;
    let at_end = *offset >= max_offset;

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
        *offset = (*offset - SCROLL_STEP).max(0.0);
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
        *offset = (*offset + SCROLL_STEP).min(max_offset);
    }
}

/// Result from the shared pin/color submenu.
enum PinColorResult {
    /// Toggle pin state.
    TogglePin,
    /// Set (or clear) the tab color.
    SetColor(Option<TabColor>),
}

/// Renders the "Pin/Unpin" toggle and "Set Tab Color" submenu shared by
/// both the main and pane context menus.
fn render_pin_color_menu_items(ui: &mut egui::Ui, is_pinned: bool) -> Option<PinColorResult> {
    let mut result = None;
    let pin_label = if is_pinned { "Unpin Tab" } else { "Pin Tab" };
    if ui.button(pin_label).clicked() {
        result = Some(PinColorResult::TogglePin);
        ui.close();
    }
    ui.menu_button("Set Tab Color", |ui| {
        for variant in TabColor::ALL {
            let [r, g, b] = variant.to_rgb();
            let label = RichText::new(variant.label()).color(Color32::from_rgb(r, g, b));
            if ui.button(label).clicked() {
                result = Some(PinColorResult::SetColor(Some(variant)));
                ui.close();
            }
        }
        ui.separator();
        if ui.button("Clear Color").clicked() {
            result = Some(PinColorResult::SetColor(None));
            ui.close();
        }
    });
    result
}

/// Renders the `Copy Path` submenu for a tab and emits the chosen request
/// into `out`. Shared by the single-pane and per-pane tab context menus.
///
/// The submenu is rendered **disabled** when `file_path` is `None` (an unsaved
/// scratch buffer has no path): the affordance stays visible and consistent
/// across saved/unsaved tabs without offering a dead click. `relative_root` is
/// `Some` only when the file lives under an open workspace folder, which gates
/// the `Relative Path` item (see [`copy_path_menu`]).
fn tab_copy_path_menu(
    ui: &mut egui::Ui,
    file_path: Option<&Path>,
    relative_root: Option<&Path>,
    out: &mut Option<CopyPathRequest>,
) {
    match file_path {
        Some(path) => copy_path_menu(ui, path, relative_root, out),
        None => {
            ui.add_enabled_ui(false, |ui| {
                copy_path_menu(ui, Path::new(""), None, out);
            });
        }
    }
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
        // departure is deliberately NOT treated as cancel, for accessibility:
        // users who cannot hold a straight horizontal line must not lose an
        // in-progress drag.
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
                    self.tab_scroll_offset = auto_scroll_offset(
                        active_rect,
                        scroll_output.inner_rect,
                        scroll_output.content_size.x,
                        self.tab_scroll_offset,
                    );
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
                    DeferredTabAction::CopyPath(req) => {
                        self.handle_copy_path(&req.path, &req.root, req.scope);
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
        let is_drag_source = self.tab_drag.is_some_and(|d| d.source_idx == idx);

        let (title_galley, title_color, tab_size) = layout_tab_title(ui, visuals, doc, is_active);
        let title_for_widget_info = title_galley.text().to_owned();
        let accent = resolve_accent_color(doc.tab_color, is_active, self.theme_ctrl.accent_color);

        // -- Allocate the single rect for the entire tab --
        let (tab_rect, response) = ui.allocate_exact_size(tab_size, Sense::click_and_drag());
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &title_for_widget_info)
        });
        let is_hovered = response.hovered();

        let chrome = TabChrome {
            tab_rect,
            title_galley,
            title_color,
            is_active,
            is_hovered,
            is_drag_source,
            accent,
        };
        let (close_rect, pointer_in_close) = paint_tab_chrome(ui, visuals, &chrome);

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
            ui.painter().line_segment(
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

    /// Renders the left/right scroll arrow buttons for the main tab bar.
    fn render_scroll_arrows(&mut self, ui: &mut egui::Ui, visuals: &Visuals, max_offset: f32) {
        render_scroll_arrow_pair(ui, visuals, &mut self.tab_scroll_offset, max_offset);
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
        // Capture path + relative root before the menu closure: computing the
        // containing workspace root reads `self.workspace_sidebar`, which would
        // conflict with the `&mut self` borrow held across `context_menu`.
        let file_path = self.tabs.documents[idx].file_path.clone();
        let relative_root = file_path.as_deref().and_then(|p| {
            copy_path_root_for(&self.workspace_sidebar.tree, p).map(Path::to_path_buf)
        });
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
            let mut copy_path_req = None;
            tab_copy_path_menu(
                ui,
                file_path.as_deref(),
                relative_root.as_deref(),
                &mut copy_path_req,
            );
            if let Some(req) = copy_path_req {
                *deferred_action = Some(DeferredTabAction::CopyPath(req));
            }
            ui.separator();
            if let Some(result) = render_pin_color_menu_items(ui, is_pinned) {
                *deferred_action = Some(match result {
                    PinColorResult::TogglePin => {
                        if is_pinned {
                            DeferredTabAction::Unpin(idx)
                        } else {
                            DeferredTabAction::Pin(idx)
                        }
                    }
                    PinColorResult::SetColor(color) => DeferredTabAction::SetTabColor(idx, color),
                });
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

    /// Renders a pane-aware tab strip showing only the documents owned by
    /// `pane`. Used by [`App::render_split_panes`] when split view is active.
    ///
    /// Includes horizontal scroll support with overflow detection, scroll
    /// arrows, and auto-scroll to active tab — mirroring the main tab bar.
    pub(crate) fn show_pane_tab_bar(&mut self, ui: &mut egui::Ui, pane: PaneId) {
        let visuals = ui.visuals().clone();
        let order = self.tabs.pane_tab_order(pane);
        let active_doc = self.tabs.pane_active_doc(pane);
        let mut actions = PaneTabActions::default();

        let pane_idx = match pane {
            PaneId::Left => 0,
            PaneId::Right => 1,
        };

        // Detect active tab change for auto-scroll.
        let need_auto_scroll = self.prev_pane_active[pane_idx] != active_doc;
        self.prev_pane_active[pane_idx] = active_doc;

        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            let mut tab_rects: Vec<Rect> = Vec::with_capacity(order.len());

            let arrows_width = if self.pane_tabs_overflow[pane_idx] {
                ARROW_BUTTON_WIDTH * 2.0
            } else {
                0.0
            };
            let new_tab_btn_width = 24.0;
            let reserved = arrows_width + new_tab_btn_width;
            let scroll_max_width = (ui.available_width() - reserved).max(0.0);

            ui.style_mut().always_scroll_the_only_direction = true;
            let scroll_output = ScrollArea::horizontal()
                .id_salt(format!("pane_tab_scroll_{pane_idx}"))
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                .horizontal_scroll_offset(self.pane_tab_scroll_offset[pane_idx])
                .max_width(scroll_max_width)
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    for &doc_idx in &order {
                        let rect = self.render_pane_tab_button(
                            ui,
                            doc_idx,
                            active_doc,
                            &visuals,
                            &mut actions,
                        );
                        tab_rects.push(rect);
                    }
                });

            // Update scroll state.
            self.pane_tab_scroll_offset[pane_idx] = scroll_output.state.offset.x;
            self.pane_tabs_overflow[pane_idx] =
                scroll_output.content_size.x > scroll_output.inner_rect.width();

            // Auto-scroll to active tab on change.
            if need_auto_scroll {
                if let Some(pos) = order.iter().position(|&idx| idx == active_doc) {
                    if let Some(active_rect) = tab_rects.get(pos).copied() {
                        self.pane_tab_scroll_offset[pane_idx] = auto_scroll_offset(
                            active_rect,
                            scroll_output.inner_rect,
                            scroll_output.content_size.x,
                            self.pane_tab_scroll_offset[pane_idx],
                        );
                    }
                }
            }

            // Scroll arrows when overflow.
            if self.pane_tabs_overflow[pane_idx] {
                let max_offset =
                    (scroll_output.content_size.x - scroll_output.inner_rect.width()).max(0.0);
                render_scroll_arrow_pair(
                    ui,
                    &visuals,
                    &mut self.pane_tab_scroll_offset[pane_idx],
                    max_offset,
                );
            }

            render_pane_new_tab_button(ui, &visuals, &mut actions);
        });

        self.apply_pane_tab_actions(pane, actions);
    }

    /// Renders a single tab button for the per-pane tab bar.
    ///
    /// Returns the allocated `Rect` for auto-scroll calculations.
    /// Updates `actions` rather than mutating tab state directly so the
    /// caller can defer all mutations until after the rendering loop has
    /// stopped iterating over `pane_tab_order`.
    fn render_pane_tab_button(
        &mut self,
        ui: &mut egui::Ui,
        doc_idx: usize,
        active_doc: usize,
        visuals: &Visuals,
        actions: &mut PaneTabActions,
    ) -> Rect {
        let doc = &self.tabs.documents[doc_idx];
        let is_active = doc_idx == active_doc;
        let (title_galley, title_color, tab_size) = layout_tab_title(ui, visuals, doc, is_active);
        let accent = resolve_accent_color(doc.tab_color, is_active, self.theme_ctrl.accent_color);

        let (tab_rect, response) = ui.allocate_exact_size(tab_size, Sense::click());
        let is_hovered = response.hovered();

        let chrome = TabChrome {
            tab_rect,
            title_galley,
            title_color,
            is_active,
            is_hovered,
            is_drag_source: false,
            accent,
        };
        let (_close_rect, pointer_in_close) = paint_tab_chrome(ui, visuals, &chrome);

        if response.clicked() {
            if pointer_in_close && (is_active || is_hovered) {
                actions.tab_to_close = Some(doc_idx);
            } else {
                actions.switch_to = Some(doc_idx);
            }
        }
        if response.middle_clicked() {
            actions.tab_to_close = Some(doc_idx);
        }

        // Context menu mirrors the single-pane tab bar's per-tab actions
        // plus "Move to Other Pane". Bulk-close actions are deliberately
        // omitted since they don't have an obvious pane scope in v1.
        // Path + relative root captured here (with `&self`) so the free-fn
        // menu builder needn't borrow `self`.
        let is_pinned = self.tabs.documents[doc_idx].pinned;
        let file_path = self.tabs.documents[doc_idx].file_path.clone();
        let relative_root = file_path.as_deref().and_then(|p| {
            copy_path_root_for(&self.workspace_sidebar.tree, p).map(Path::to_path_buf)
        });
        render_pane_tab_context_menu(
            doc_idx,
            is_pinned,
            file_path.as_deref(),
            relative_root.as_deref(),
            &response,
            actions,
        );

        tab_rect
    }

    /// Applies the deferred actions collected during the per-pane tab bar
    /// render pass.
    ///
    /// Kept separate so that the rendering loop never mutates the document
    /// list while iterating, and so the orchestrator function stays under
    /// SonarCloud's cognitive complexity threshold.
    fn apply_pane_tab_actions(&mut self, pane: PaneId, actions: PaneTabActions) {
        let PaneTabActions {
            switch_to,
            tab_to_close,
            move_to_other,
            pin_action,
            color_action,
            new_tab_in_pane,
            copy_path,
        } = actions;

        if let Some(idx) = switch_to {
            self.tabs.switch_pane_to(pane, idx);
            self.tabs.focus_pane(pane);
        }
        if let Some(idx) = tab_to_close {
            self.tabs.focus_pane(pane);
            self.request_close_tab(idx);
        }
        if let Some(idx) = move_to_other {
            self.tabs.move_tab_to_pane(idx, pane.other());
        }
        if let Some((idx, pin)) = pin_action {
            self.apply_pin_action(idx, pin);
        }
        if let Some((idx, color)) = color_action {
            if idx < self.tabs.documents.len() {
                self.tabs.documents[idx].tab_color = color;
            }
        }
        if new_tab_in_pane {
            self.tabs.focus_pane(pane);
            self.new_tab();
        }
        if let Some(req) = copy_path {
            self.handle_copy_path(&req.path, &req.root, req.scope);
        }
    }

    /// Pins or unpins the document at `idx` based on `pin`.
    fn apply_pin_action(&mut self, idx: usize, pin: bool) {
        if pin {
            self.tabs.pin_tab(idx);
        } else {
            self.tabs.unpin_tab(idx);
        }
    }
}

/// Deferred actions collected while rendering a per-pane tab bar.
///
/// Mutating tab state during the render loop would invalidate the
/// `pane_tab_order` indices, so the loop only writes into this struct and
/// the caller drains it once rendering finishes.
#[derive(Default)]
struct PaneTabActions {
    switch_to: Option<usize>,
    tab_to_close: Option<usize>,
    move_to_other: Option<usize>,
    pin_action: Option<(usize, bool)>,
    color_action: Option<(usize, Option<TabColor>)>,
    new_tab_in_pane: bool,
    copy_path: Option<CopyPathRequest>,
}

/// Renders the right-click context menu for the per-pane tab bar.
fn render_pane_tab_context_menu(
    doc_idx: usize,
    is_pinned: bool,
    file_path: Option<&Path>,
    relative_root: Option<&Path>,
    response: &egui::Response,
    actions: &mut PaneTabActions,
) {
    response.context_menu(|ui| {
        if ui.button("Close").clicked() {
            actions.tab_to_close = Some(doc_idx);
            ui.close();
        }
        if ui.button("Move to Other Pane").clicked() {
            actions.move_to_other = Some(doc_idx);
            ui.close();
        }
        ui.separator();
        tab_copy_path_menu(ui, file_path, relative_root, &mut actions.copy_path);
        ui.separator();
        if let Some(result) = render_pin_color_menu_items(ui, is_pinned) {
            match result {
                PinColorResult::TogglePin => {
                    actions.pin_action = Some((doc_idx, !is_pinned));
                }
                PinColorResult::SetColor(color) => {
                    actions.color_action = Some((doc_idx, color));
                }
            }
        }
    });
}

/// Renders the trailing "+" button (and double-click drop-zone) for the
/// per-pane tab bar.
fn render_pane_new_tab_button(ui: &mut egui::Ui, visuals: &Visuals, actions: &mut PaneTabActions) {
    ui.spacing_mut().item_spacing.x = 4.0;
    let new_btn = egui::Button::new(
        RichText::new("+")
            .color(visuals.widgets.noninteractive.fg_stroke.color)
            .size(16.0),
    )
    .fill(Color32::TRANSPARENT)
    .stroke(Stroke::NONE);
    if ui.add(new_btn).clicked() {
        actions.new_tab_in_pane = true;
    }

    // Double-click on the empty area to the right of the "+" button also
    // opens a new tab — matches the single-pane bar's behavior in
    // `render_empty_tab_bar_area`.
    let remaining = ui.available_size();
    if remaining.x > 0.0 {
        let empty = ui.allocate_response(remaining, Sense::click());
        if empty.double_clicked() {
            actions.new_tab_in_pane = true;
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

    // ── format_tab_title ────────────────────────────────────────────

    #[test]
    fn format_tab_title_plain() {
        assert_eq!(format_tab_title(false, false, "main.rs"), "main.rs");
    }

    #[test]
    fn format_tab_title_modified_only() {
        assert_eq!(format_tab_title(false, true, "main.rs"), "main.rs *");
    }

    #[test]
    fn format_tab_title_pinned_only() {
        assert_eq!(
            format_tab_title(true, false, "main.rs"),
            "\u{1F4CC} main.rs"
        );
    }

    #[test]
    fn format_tab_title_pinned_and_modified() {
        assert_eq!(
            format_tab_title(true, true, "main.rs"),
            "\u{1F4CC} main.rs *"
        );
    }

    #[test]
    fn format_tab_title_handles_empty_title() {
        // Edge case: an unsaved scratch buffer may carry an empty title.
        assert_eq!(format_tab_title(false, false, ""), "");
        assert_eq!(format_tab_title(false, true, ""), " *");
        assert_eq!(format_tab_title(true, false, ""), "\u{1F4CC} ");
    }

    // ── compute_tab_width ───────────────────────────────────────────

    #[test]
    fn compute_tab_width_includes_padding_and_close_area() {
        // 100 (title) + 8+8 padding + 4 gap + 14 close area = 134.
        assert_eq!(compute_tab_width(100.0), 134.0);
    }

    #[test]
    fn compute_tab_width_minimum_with_zero_title() {
        // With no title, width is just padding + gap + close area.
        assert_eq!(compute_tab_width(0.0), 34.0);
    }

    // ── resolve_accent_color ────────────────────────────────────────

    #[test]
    fn resolve_accent_color_uses_explicit_tab_color_for_inactive() {
        let active_accent = Color32::from_rgb(10, 20, 30);
        let resolved = resolve_accent_color(Some(TabColor::Red), false, active_accent);
        let [r, g, b] = TabColor::Red.to_rgb();
        assert_eq!(resolved, Some(Color32::from_rgb(r, g, b)));
    }

    #[test]
    fn resolve_accent_color_explicit_overrides_active_theme() {
        let active_accent = Color32::from_rgb(10, 20, 30);
        let resolved = resolve_accent_color(Some(TabColor::Blue), true, active_accent);
        let [r, g, b] = TabColor::Blue.to_rgb();
        assert_eq!(resolved, Some(Color32::from_rgb(r, g, b)));
    }

    #[test]
    fn resolve_accent_color_falls_back_to_theme_when_active() {
        let active_accent = Color32::from_rgb(10, 20, 30);
        assert_eq!(
            resolve_accent_color(None, true, active_accent),
            Some(active_accent)
        );
    }

    #[test]
    fn resolve_accent_color_none_when_inactive_without_color() {
        let active_accent = Color32::from_rgb(10, 20, 30);
        assert_eq!(resolve_accent_color(None, false, active_accent), None);
    }

    // ── title_color_for ─────────────────────────────────────────────

    #[test]
    fn title_color_for_active_dark_mode() {
        let visuals = Visuals::dark();
        assert_eq!(
            title_color_for(true, &visuals),
            Color32::from_rgb(220, 220, 220)
        );
    }

    #[test]
    fn title_color_for_active_light_mode() {
        let visuals = Visuals::light();
        assert_eq!(
            title_color_for(true, &visuals),
            Color32::from_rgb(30, 30, 30)
        );
    }

    #[test]
    fn title_color_for_inactive_uses_noninteractive_fg() {
        let visuals = Visuals::dark();
        assert_eq!(
            title_color_for(false, &visuals),
            visuals.widgets.noninteractive.fg_stroke.color
        );
    }

    // ── PaneTabActions ──────────────────────────────────────────────

    #[test]
    fn pane_tab_actions_default_is_empty() {
        let a = PaneTabActions::default();
        assert!(a.switch_to.is_none());
        assert!(a.tab_to_close.is_none());
        assert!(a.move_to_other.is_none());
        assert!(a.pin_action.is_none());
        assert!(a.color_action.is_none());
        assert!(!a.new_tab_in_pane);
        assert!(a.copy_path.is_none());
    }

    // ── auto_scroll_offset ─────────────────────────────────────────

    /// Helper: builds a scroll-area scenario.
    ///
    /// `inner_rect` is positioned at screen x=`inner_x` with the given
    /// `visible_width`. `content_width` is the total scrollable content
    /// width. The target tab rect has width 100 starting at screen x=`tab_screen_x`.
    fn scroll_scenario(
        inner_x: f32,
        visible_width: f32,
        content_width: f32,
        offset: f32,
        tab_screen_x: f32,
    ) -> f32 {
        let inner_rect =
            Rect::from_min_size(Pos2::new(inner_x, 0.0), Vec2::new(visible_width, 32.0));
        let target_rect = Rect::from_min_size(Pos2::new(tab_screen_x, 0.0), Vec2::new(100.0, 32.0));
        auto_scroll_offset(target_rect, inner_rect, content_width, offset)
    }

    #[test]
    fn auto_scroll_tab_already_visible_no_change() {
        // Viewport at inner_x=0, visible_width=500, offset=0.
        // Tab at screen x=100 → content_left=100, content_right=200.
        // Both within [0, 500) → no change.
        let result = scroll_scenario(0.0, 500.0, 1000.0, 0.0, 100.0);
        assert_eq!(result, 0.0);
    }

    #[test]
    fn auto_scroll_tab_left_of_viewport_scrolls_left() {
        // Viewport at inner_x=0, visible_width=500, offset=300.
        // Tab at screen x=-50 → content_left = -50 - 0 + 300 = 250 < 300.
        // Should scroll left to (250 - TAB_PADDING).
        let result = scroll_scenario(0.0, 500.0, 1000.0, 300.0, -50.0);
        assert_eq!(result, 250.0 - TAB_PADDING);
    }

    #[test]
    fn auto_scroll_tab_right_of_viewport_scrolls_right() {
        // Viewport at inner_x=0, visible_width=300, offset=0.
        // Tab at screen x=350 → content_right = 350 + 100 = 450 > 300.
        // max_offset = 1000 - 300 = 700.
        // new offset = (450 - 300 + TAB_PADDING) = 158 → clamped to 158.
        let result = scroll_scenario(0.0, 300.0, 1000.0, 0.0, 350.0);
        assert_eq!(result, (450.0 - 300.0 + TAB_PADDING).min(700.0));
    }

    #[test]
    fn auto_scroll_left_clamps_to_zero() {
        // Tab far to the left — scroll should not go negative.
        let result = scroll_scenario(0.0, 500.0, 1000.0, 10.0, -500.0);
        assert_eq!(result, 0.0);
    }

    #[test]
    fn auto_scroll_right_clamps_to_max_offset() {
        // Content barely wider than viewport.
        // content_width=310, visible_width=300 → max_offset=10.
        // Tab at screen x=305 → content_right = 405 > 300.
        // new offset = (405 - 300 + TAB_PADDING) = 113, clamped to 10.
        let result = scroll_scenario(0.0, 300.0, 310.0, 0.0, 305.0);
        assert_eq!(result, 10.0);
    }

    // ── apply_pane_tab_actions ──────────────────────────────────────

    use super::super::tests::test_app;
    use crate::tabs::PaneId;

    #[test]
    fn apply_pane_tab_actions_switch_to() {
        let mut app = test_app();
        // Create a second tab so we can switch.
        app.new_tab();
        let actions = PaneTabActions {
            switch_to: Some(0),
            ..Default::default()
        };
        app.apply_pane_tab_actions(PaneId::Left, actions);
        // switch_pane_to + focus_pane were called — verify no panic.
    }

    #[test]
    fn apply_pane_tab_actions_pin() {
        let mut app = test_app();
        let actions = PaneTabActions {
            pin_action: Some((0, true)),
            ..Default::default()
        };
        app.apply_pane_tab_actions(PaneId::Left, actions);
        assert!(app.tabs.documents[0].pinned);
    }

    #[test]
    fn apply_pane_tab_actions_unpin() {
        let mut app = test_app();
        app.tabs.documents[0].pinned = true;
        let actions = PaneTabActions {
            pin_action: Some((0, false)),
            ..Default::default()
        };
        app.apply_pane_tab_actions(PaneId::Left, actions);
        assert!(!app.tabs.documents[0].pinned);
    }

    #[test]
    fn apply_pane_tab_actions_set_color() {
        let mut app = test_app();
        let actions = PaneTabActions {
            color_action: Some((0, Some(TabColor::Red))),
            ..Default::default()
        };
        app.apply_pane_tab_actions(PaneId::Left, actions);
        assert_eq!(app.tabs.documents[0].tab_color, Some(TabColor::Red));
    }

    #[test]
    fn apply_pane_tab_actions_clear_color() {
        let mut app = test_app();
        app.tabs.documents[0].tab_color = Some(TabColor::Blue);
        let actions = PaneTabActions {
            color_action: Some((0, None)),
            ..Default::default()
        };
        app.apply_pane_tab_actions(PaneId::Left, actions);
        assert_eq!(app.tabs.documents[0].tab_color, None);
    }

    #[test]
    fn apply_pane_tab_actions_new_tab_in_pane() {
        let mut app = test_app();
        let before_count = app.tabs.tab_count();
        let actions = PaneTabActions {
            new_tab_in_pane: true,
            ..Default::default()
        };
        app.apply_pane_tab_actions(PaneId::Right, actions);
        assert_eq!(app.tabs.tab_count(), before_count + 1);
    }

    #[test]
    fn tab_copy_path_menu_renders_for_saved_and_unsaved_tabs() {
        // Render-only (no clicks): exercises both branches of the helper —
        // a saved tab (delegates to copy_path_menu) and an unsaved buffer
        // (disabled wrapper). No interaction → no request emitted.
        let ctx = egui::Context::default();
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::pos2(0.0, 0.0),
                egui::vec2(300.0, 300.0),
            )),
            ..Default::default()
        };
        let _ = ctx.run_ui(raw, |ui| {
            let mut out = None;
            // Saved + under a workspace root → all items available.
            tab_copy_path_menu(
                ui,
                Some(Path::new("/proj/a.rs")),
                Some(Path::new("/proj")),
                &mut out,
            );
            // Saved but outside any root → Relative disabled.
            tab_copy_path_menu(ui, Some(Path::new("/x/a.rs")), None, &mut out);
            // Unsaved buffer → whole submenu disabled.
            tab_copy_path_menu(ui, None, None, &mut out);
            assert!(out.is_none(), "no clicks → no request emitted");
        });
    }

    #[test]
    fn apply_pane_tab_actions_copy_path_runs_without_panic() {
        let mut app = test_app();
        // No clipboard handle in test_app — exercise the dispatch path and
        // confirm `handle_copy_path` is reached cleanly via the drained action.
        let actions = PaneTabActions {
            copy_path: Some(CopyPathRequest {
                path: std::path::PathBuf::from("/proj/src/main.rs"),
                root: std::path::PathBuf::from("/proj"),
                scope: crate::workspace::sidebar::CopyPathScope::Relative,
            }),
            ..Default::default()
        };
        app.apply_pane_tab_actions(PaneId::Left, actions);
    }

    // ── apply_pin_action ────────────────────────────────────────────

    #[test]
    fn apply_pin_action_pins_tab() {
        let mut app = test_app();
        assert!(!app.tabs.documents[0].pinned);
        app.apply_pin_action(0, true);
        assert!(app.tabs.documents[0].pinned);
    }

    #[test]
    fn apply_pin_action_unpins_tab() {
        let mut app = test_app();
        app.tabs.documents[0].pinned = true;
        app.apply_pin_action(0, false);
        assert!(!app.tabs.documents[0].pinned);
    }
}
