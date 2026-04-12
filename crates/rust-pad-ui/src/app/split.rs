//! Split-view (dual pane) rendering and toggle logic.
//!
//! This module owns:
//! - [`SplitState`], the App-level UI state for the divider (orientation,
//!   ratio, drag flag);
//! - The toggle methods that flip between single-pane and split-pane mode;
//! - [`App::render_split_panes`], the per-frame renderer that allocates two
//!   child UIs separated by a draggable divider, draws each pane's tab strip
//!   and editor, and routes pointer focus to the correct pane.
//!
//! The per-pane tab ownership lives on [`crate::tabs::TabManager`]; this
//! module only deals with how the panes are laid out and which one currently
//! has focus.

use eframe::egui;
use egui::{CursorIcon, Rect, Sense, UiBuilder};
use rust_pad_config::session::SessionSplit;

use crate::editor::EditorWidget;
use crate::tabs::{PaneId, SplitOrientation};

use super::App;

/// Minimum size in pixels reserved for each pane along the divider axis.
/// The divider drag clamps the ratio so neither pane shrinks below this.
const MIN_PANE_PIXELS: f32 = 80.0;

/// Width of the draggable divider rectangle, in pixels.
const DIVIDER_THICKNESS: f32 = 4.0;

/// UI-layer state for split view. Owned by [`App`].
#[derive(Debug, Clone)]
pub struct SplitState {
    /// How the editor area is divided.
    pub orientation: SplitOrientation,
    /// Fraction of the central panel allocated to the Left (or top) pane,
    /// in the range `[0.0, 1.0]`.
    pub divider_ratio: f32,
    /// True while the user is dragging the divider this frame. Used to
    /// suppress focus updates so a drag does not flip the focused pane.
    pub dragging_divider: bool,
}

impl Default for SplitState {
    fn default() -> Self {
        Self {
            orientation: SplitOrientation::Vertical,
            divider_ratio: 0.5,
            dragging_divider: false,
        }
    }
}

impl App {
    /// Returns true if split view is currently active.
    pub(crate) fn is_split(&self) -> bool {
        self.split.is_some() && self.tabs.is_split()
    }

    /// Enables split view with the given orientation. If split was already
    /// active, just switches the orientation. The previously active document
    /// is moved into the right pane.
    pub(crate) fn enable_split(&mut self, orientation: SplitOrientation) {
        // `tabs.panes` may have been auto-collapsed by `close_tab` /
        // `move_tab_to_pane` while our UI-side `split` lingered. Rebuild
        // the panes whenever they are missing, regardless of `self.split`.
        if !self.tabs.is_split() {
            self.tabs.enable_split();
        }
        match self.split.as_mut() {
            Some(state) => state.orientation = orientation,
            None => {
                self.split = Some(SplitState {
                    orientation,
                    divider_ratio: 0.5,
                    dragging_divider: false,
                });
            }
        }
    }

    /// Disables split view, returning to a single editor pane.
    pub(crate) fn remove_split(&mut self) {
        self.split = None;
        self.tabs.disable_split();
    }

    /// Drops the UI-side `split` state when `tabs.panes` has been
    /// auto-collapsed underneath us. Without this, the next toggle would
    /// see stale `Some(...)` state and treat the toggle as "remove split"
    /// — making the user click twice to re-enable the split.
    fn normalize_split_state(&mut self) {
        if self.split.is_some() && !self.tabs.is_split() {
            self.split = None;
        }
    }

    /// Toggles split view on with vertical orientation, or off if already
    /// active with the same orientation.
    pub(crate) fn toggle_split_vertical(&mut self) {
        self.normalize_split_state();
        match self.split.as_ref() {
            Some(s) if s.orientation == SplitOrientation::Vertical => self.remove_split(),
            _ => self.enable_split(SplitOrientation::Vertical),
        }
    }

    /// Toggles split view on with horizontal orientation, or off if already
    /// active with the same orientation.
    pub(crate) fn toggle_split_horizontal(&mut self) {
        self.normalize_split_state();
        match self.split.as_ref() {
            Some(s) if s.orientation == SplitOrientation::Horizontal => self.remove_split(),
            _ => self.enable_split(SplitOrientation::Horizontal),
        }
    }

    /// Builds a [`SessionSplit`] snapshot of the current split state, or
    /// `None` when split view is not active. Called from `on_exit`.
    pub(crate) fn build_session_split(&self) -> Option<SessionSplit> {
        let state = self.split.as_ref()?;
        let panes = self.tabs.panes.as_ref()?;

        // The session stores positions inside `SessionData::tabs` (which
        // mirrors `tabs.documents` 1:1 at save time), so we can use the
        // raw document indices directly.
        let left_active_pos = panes
            .left_order
            .iter()
            .position(|&i| i == panes.left_active)
            .unwrap_or(0);
        let right_active_pos = panes
            .right_order
            .iter()
            .position(|&i| i == panes.right_active)
            .unwrap_or(0);

        Some(SessionSplit {
            orientation: match state.orientation {
                SplitOrientation::Vertical => "vertical".to_string(),
                SplitOrientation::Horizontal => "horizontal".to_string(),
            },
            divider_ratio: state.divider_ratio,
            left_tab_indices: panes.left_order.clone(),
            right_tab_indices: panes.right_order.clone(),
            left_active: left_active_pos,
            right_active: right_active_pos,
            focused: match panes.focused {
                PaneId::Left => "left".to_string(),
                PaneId::Right => "right".to_string(),
            },
        })
    }

    /// Applies a persisted [`SessionSplit`] to the current `App` and
    /// `TabManager`. Indices in `split` refer to positions inside the
    /// already-restored `tabs.documents` vector.
    pub(crate) fn apply_session_split(&mut self, split: &SessionSplit) {
        let n = self.tabs.documents.len();
        if n == 0 {
            return;
        }

        // Validate every index. If anything is out of range, drop the
        // split entirely rather than corrupt the running state.
        let valid = split.left_tab_indices.iter().all(|&i| i < n)
            && split.right_tab_indices.iter().all(|&i| i < n)
            && !split.left_tab_indices.is_empty()
            && !split.right_tab_indices.is_empty();
        if !valid {
            return;
        }

        let orientation = match split.orientation.as_str() {
            "horizontal" => SplitOrientation::Horizontal,
            _ => SplitOrientation::Vertical,
        };
        let focused = match split.focused.as_str() {
            "left" => PaneId::Left,
            _ => PaneId::Right,
        };

        let left_active_doc = split
            .left_tab_indices
            .get(split.left_active)
            .copied()
            .unwrap_or(split.left_tab_indices[0]);
        let right_active_doc = split
            .right_tab_indices
            .get(split.right_active)
            .copied()
            .unwrap_or(split.right_tab_indices[0]);

        // Install the pane assignment directly. We bypass `enable_split`
        // because we want to honor the persisted layout exactly.
        self.tabs.panes = Some(crate::tabs::PaneTabSplit {
            left_order: split.left_tab_indices.clone(),
            right_order: split.right_tab_indices.clone(),
            left_active: left_active_doc,
            right_active: right_active_doc,
            focused,
        });
        self.tabs.active = match focused {
            PaneId::Left => left_active_doc,
            PaneId::Right => right_active_doc,
        };

        self.split = Some(SplitState {
            orientation,
            divider_ratio: split.divider_ratio.clamp(0.05, 0.95),
            dragging_divider: false,
        });
    }

    /// Renders the central panel content as two side-by-side (or stacked)
    /// panes separated by a draggable divider. Called from the central panel
    /// closure when [`App::is_split`] is true.
    pub(crate) fn render_split_panes(&mut self, ui: &mut egui::Ui, dialog_open: bool) {
        let Some(state) = self.split.as_ref() else {
            return;
        };
        let orientation = state.orientation;
        let mut ratio = state.divider_ratio;

        let outer = ui.max_rect();

        // ── Attach a drag sense to the divider rect WITHOUT advancing the
        //    parent UI's layout cursor. Using `allocate_rect` here would
        //    push the cursor past a wide horizontal divider strip in
        //    horizontal-split mode, which interfered with the child pane
        //    UIs created via `scope_builder` and prevented the split from
        //    rendering on the frame it was first enabled. ────────────────
        let (_, divider_rect, _) = split_rects(outer, orientation, ratio, DIVIDER_THICKNESS);
        let divider_id = ui.id().with("split_view_divider");
        let divider_response = ui.interact(divider_rect, divider_id, Sense::click_and_drag());
        let divider_color = ui.visuals().widgets.noninteractive.bg_stroke.color;
        ui.painter().rect_filled(divider_rect, 0.0, divider_color);

        let cursor = match orientation {
            SplitOrientation::Vertical => CursorIcon::ResizeHorizontal,
            SplitOrientation::Horizontal => CursorIcon::ResizeVertical,
        };
        if divider_response.hovered() || divider_response.dragged() {
            ui.ctx().set_cursor_icon(cursor);
        }

        let mut dragging = false;
        if divider_response.dragged() {
            let delta = divider_response.drag_delta();
            let (along_delta, along_total) = match orientation {
                SplitOrientation::Vertical => (delta.x, outer.width()),
                SplitOrientation::Horizontal => (delta.y, outer.height()),
            };
            if along_total > 0.0 {
                ratio += along_delta / along_total;
                let min_ratio = (MIN_PANE_PIXELS / along_total).clamp(0.05, 0.45);
                ratio = ratio.clamp(min_ratio, 1.0 - min_ratio);
            }
            dragging = true;
        }
        if divider_response.double_clicked() {
            ratio = 0.5;
        }

        if let Some(s) = self.split.as_mut() {
            s.divider_ratio = ratio;
            s.dragging_divider = dragging;
        }

        // Recompute pane rects in case the ratio changed this frame.
        let (left_rect, _, right_rect) = split_rects(outer, orientation, ratio, DIVIDER_THICKNESS);

        // ── Detect press-to-focus. We check the press_origin against the
        //    pane rects, but only when the divider is not being dragged. ──
        if !dragging {
            if let Some(press) = ui.input(|i| i.pointer.press_origin()) {
                if ui.input(|i| i.pointer.any_pressed()) {
                    if left_rect.contains(press) {
                        self.tabs.focus_pane(PaneId::Left);
                    } else if right_rect.contains(press) {
                        self.tabs.focus_pane(PaneId::Right);
                    }
                }
            }
        }

        // ── Render each pane in its own child UI. ──────────────────────
        let zoom_left = self.render_one_pane(ui, PaneId::Left, left_rect, dialog_open);
        let zoom_right = self.render_one_pane(ui, PaneId::Right, right_rect, dialog_open);
        let zoom_request = if zoom_left != 1.0 {
            zoom_left
        } else {
            zoom_right
        };

        if zoom_request != 1.0 {
            self.theme_ctrl.zoom_level = (self.theme_ctrl.zoom_level * zoom_request)
                .clamp(0.5, self.theme_ctrl.max_zoom_level);
        }
    }

    /// Renders the tab strip and editor for one pane inside a child UI
    /// clipped to `pane_rect`. Returns the zoom request from the editor
    /// (1.0 if no zoom change).
    fn render_one_pane(
        &mut self,
        parent: &mut egui::Ui,
        pane: PaneId,
        pane_rect: Rect,
        dialog_open: bool,
    ) -> f32 {
        let mut zoom_request = 1.0_f32;
        let focused = self.tabs.focused_pane() == pane;
        let accent = self.theme_ctrl.accent_color;

        parent.scope_builder(
            UiBuilder::new()
                .max_rect(pane_rect)
                .layout(egui::Layout::top_down(egui::Align::LEFT)),
            |ui| {
                ui.set_clip_rect(pane_rect);

                // Pane-aware tab strip.
                self.show_pane_tab_bar(ui, pane);

                // Editor for this pane's active document.
                let response = {
                    let doc = self.tabs.pane_active_doc_mut(pane);
                    let mut editor = EditorWidget::new(
                        doc,
                        &self.theme_ctrl.theme,
                        self.theme_ctrl.zoom_level,
                        Some(&self.theme_ctrl.syntax_highlighter),
                    );
                    editor.word_wrap = self.word_wrap;
                    editor.show_special_chars = self.show_special_chars;
                    editor.show_line_numbers = self.show_line_numbers;
                    editor.dialog_open = dialog_open;
                    editor.bookmarks = Some(&self.bookmarks);
                    let r = editor.show(ui);
                    zoom_request = editor.zoom_request;
                    r
                };
                response.context_menu(|ui| {
                    self.show_editor_context_menu(ui);
                });

                // Focused-pane indicator: 1px accent border around the pane.
                if focused {
                    ui.painter().rect_stroke(
                        pane_rect.shrink(0.5),
                        0.0,
                        egui::Stroke::new(1.0, accent),
                        egui::StrokeKind::Inside,
                    );
                }
            },
        );

        zoom_request
    }
}

/// Splits the outer rect into `(pane_a, divider, pane_b)` for the given
/// orientation. `ratio` is the fraction of the outer extent (along the
/// divider axis) allocated to the first pane.
fn split_rects(
    outer: Rect,
    orientation: SplitOrientation,
    ratio: f32,
    divider: f32,
) -> (Rect, Rect, Rect) {
    let ratio = ratio.clamp(0.0, 1.0);
    match orientation {
        SplitOrientation::Vertical => {
            let total = outer.width();
            let left_w = (total * ratio).round();
            let left = Rect::from_min_max(outer.min, egui::pos2(outer.min.x + left_w, outer.max.y));
            let div = Rect::from_min_max(
                egui::pos2(left.max.x, outer.min.y),
                egui::pos2(left.max.x + divider, outer.max.y),
            );
            let right = Rect::from_min_max(egui::pos2(div.max.x, outer.min.y), outer.max);
            (left, div, right)
        }
        SplitOrientation::Horizontal => {
            let total = outer.height();
            let top_h = (total * ratio).round();
            let top = Rect::from_min_max(outer.min, egui::pos2(outer.max.x, outer.min.y + top_h));
            let div = Rect::from_min_max(
                egui::pos2(outer.min.x, top.max.y),
                egui::pos2(outer.max.x, top.max.y + divider),
            );
            let bottom = Rect::from_min_max(egui::pos2(outer.min.x, div.max.y), outer.max);
            (top, div, bottom)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::pos2;

    #[test]
    fn split_rects_vertical_50_50_partitions_outer() {
        let outer = Rect::from_min_max(pos2(0.0, 0.0), pos2(100.0, 80.0));
        let (l, d, r) = split_rects(outer, SplitOrientation::Vertical, 0.5, 4.0);
        assert_eq!(l.min, pos2(0.0, 0.0));
        assert_eq!(l.max, pos2(50.0, 80.0));
        assert_eq!(d.min, pos2(50.0, 0.0));
        assert_eq!(d.max, pos2(54.0, 80.0));
        assert_eq!(r.min, pos2(54.0, 0.0));
        assert_eq!(r.max, pos2(100.0, 80.0));
    }

    #[test]
    fn split_rects_horizontal_assigns_top_first() {
        let outer = Rect::from_min_max(pos2(0.0, 0.0), pos2(100.0, 80.0));
        let (top, _d, bottom) = split_rects(outer, SplitOrientation::Horizontal, 0.5, 4.0);
        assert!(top.max.y < bottom.min.y);
        assert_eq!(top.min, outer.min);
        assert_eq!(bottom.max, outer.max);
    }

    #[test]
    fn split_rects_clamped_ratio_keeps_panes_valid() {
        let outer = Rect::from_min_max(pos2(0.0, 0.0), pos2(200.0, 80.0));
        // The caller is responsible for clamping the ratio so the divider
        // fits inside `outer`. Verify the typical clamped range produces
        // non-inverted rects on both sides.
        for ratio in [0.1_f32, 0.25, 0.5, 0.75, 0.9] {
            let (l, d, r) = split_rects(outer, SplitOrientation::Vertical, ratio, 4.0);
            assert!(l.min.x <= l.max.x, "left inverted at ratio {ratio}");
            assert!(d.min.x <= d.max.x, "divider inverted at ratio {ratio}");
            assert!(r.min.x <= r.max.x, "right inverted at ratio {ratio}");
        }
    }

    #[test]
    fn toggle_split_horizontal_from_single_pane_enables_horizontal() {
        let mut app = super::super::tests::test_app();
        assert!(!app.is_split());
        app.toggle_split_horizontal();
        assert!(app.is_split(), "split should be active after one toggle");
        assert_eq!(
            app.split.as_ref().unwrap().orientation,
            SplitOrientation::Horizontal,
            "single click should land in Horizontal orientation"
        );
    }

    #[test]
    fn toggle_split_vertical_from_single_pane_enables_vertical() {
        let mut app = super::super::tests::test_app();
        assert!(!app.is_split());
        app.toggle_split_vertical();
        assert!(app.is_split());
        assert_eq!(
            app.split.as_ref().unwrap().orientation,
            SplitOrientation::Vertical
        );
    }

    #[test]
    fn second_horizontal_toggle_removes_split() {
        let mut app = super::super::tests::test_app();
        app.toggle_split_horizontal();
        assert!(app.is_split());
        app.toggle_split_horizontal();
        assert!(!app.is_split(), "second toggle should collapse the split");
    }

    #[test]
    fn toggle_recovers_from_auto_collapsed_panes() {
        // Reproduces the "needs two clicks" bug: if `tabs.panes` is
        // auto-collapsed (e.g. by closing the last tab in a pane) while
        // the UI-side `app.split` is left dangling as `Some(...)`, the
        // next toggle must still enable the split on the FIRST click.
        let mut app = super::super::tests::test_app();
        app.toggle_split_horizontal();
        assert!(app.is_split());

        // Simulate the auto-collapse path: drop pane state directly while
        // leaving `app.split` populated. This mirrors what `close_tab`
        // does when a pane becomes empty.
        app.tabs.disable_split();
        assert!(app.split.is_some());
        assert!(!app.is_split(), "is_split should observe the collapse");

        // First toggle after the inconsistency must enable split, not
        // silently swallow the click.
        app.toggle_split_horizontal();
        assert!(
            app.is_split(),
            "first toggle after auto-collapse must re-enable the split"
        );
        assert_eq!(
            app.split.as_ref().unwrap().orientation,
            SplitOrientation::Horizontal
        );
    }

    #[test]
    fn toggle_vertical_recovers_from_auto_collapsed_panes() {
        let mut app = super::super::tests::test_app();
        app.toggle_split_vertical();
        assert!(app.is_split());

        app.tabs.disable_split();
        assert!(app.split.is_some());
        assert!(!app.is_split());

        app.toggle_split_vertical();
        assert!(
            app.is_split(),
            "first toggle after auto-collapse must re-enable the split"
        );
        assert_eq!(
            app.split.as_ref().unwrap().orientation,
            SplitOrientation::Vertical
        );
    }

    #[test]
    fn build_then_apply_session_split_round_trips() {
        // Set up an app with several tabs and a non-trivial split layout,
        // serialize via `build_session_split`, then apply that snapshot to
        // a fresh app and assert the layouts match.
        let mut app = super::super::tests::test_app();
        app.tabs.new_tab(); // doc 1
        app.tabs.new_tab(); // doc 2
        app.tabs.new_tab(); // doc 3
        assert_eq!(app.tabs.tab_count(), 4);
        app.tabs.switch_to(1);
        app.toggle_split_horizontal();
        // After enable_split with multi-tab: left=[0,2,3], right=[1],
        // focused=Right.
        app.tabs.focus_pane(PaneId::Left);

        if let Some(s) = app.split.as_mut() {
            s.divider_ratio = 0.42;
        }

        let snapshot = app.build_session_split().expect("split should be active");
        assert_eq!(snapshot.orientation, "horizontal");
        assert_eq!(snapshot.focused, "left");

        // Now build a fresh app with the same number of documents and
        // apply the snapshot.
        let mut restored = super::super::tests::test_app();
        restored.tabs.new_tab();
        restored.tabs.new_tab();
        restored.tabs.new_tab();
        assert_eq!(restored.tabs.tab_count(), 4);
        restored.apply_session_split(&snapshot);

        assert!(restored.is_split());
        let r_panes = restored.tabs.panes.as_ref().unwrap();
        let o_panes = app.tabs.panes.as_ref().unwrap();
        assert_eq!(r_panes.left_order, o_panes.left_order);
        assert_eq!(r_panes.right_order, o_panes.right_order);
        assert_eq!(r_panes.focused, o_panes.focused);
        assert_eq!(
            restored.split.as_ref().unwrap().orientation,
            SplitOrientation::Horizontal
        );
        assert!((restored.split.as_ref().unwrap().divider_ratio - 0.42).abs() < 1e-6);
    }

    #[test]
    fn apply_session_split_rejects_out_of_range_indices() {
        let mut app = super::super::tests::test_app();
        app.tabs.new_tab(); // 2 docs total
        let bad_snapshot = SessionSplit {
            orientation: "vertical".to_string(),
            divider_ratio: 0.5,
            left_tab_indices: vec![0],
            right_tab_indices: vec![99], // out of range
            left_active: 0,
            right_active: 0,
            focused: "right".to_string(),
        };
        app.apply_session_split(&bad_snapshot);
        assert!(
            !app.is_split(),
            "applying an out-of-range snapshot must not enable split"
        );
    }
}
