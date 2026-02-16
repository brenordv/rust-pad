//! Scrollbar rendering and interaction for the editor widget.
//!
//! Handles vertical and horizontal scrollbar track/thumb rendering,
//! drag interaction, and click-to-jump behavior.

use egui::{Pos2, Rect, Response, Ui, Vec2};
use rust_pad_core::document::ScrollbarDrag;

use super::widget::{EditorWidget, SCROLLBAR_MIN_THUMB, SCROLLBAR_WIDTH};

/// Resolves the thumb color based on drag/hover state.
fn thumb_color(
    theme: &super::theme::EditorTheme,
    is_dragging: bool,
    is_hovering: bool,
) -> egui::Color32 {
    if is_dragging {
        theme.scrollbar_thumb_active
    } else if is_hovering {
        theme.scrollbar_thumb_hover
    } else {
        theme.scrollbar_thumb_idle
    }
}

/// Handles drag and click-to-jump interaction for a single scrollbar axis.
///
/// `scroll` is the mutable scroll position for this axis.
/// `axis` extracts the relevant coordinate (x or y) from a pointer position.
#[allow(clippy::too_many_arguments)]
fn handle_axis_scroll(
    scroll: &mut f32,
    response: &Response,
    is_dragging: bool,
    track_rect: Rect,
    thumb_size: f32,
    thumb_travel: f32,
    max_scroll: f32,
    axis: fn(Pos2) -> f32,
) {
    let track_start = axis(track_rect.min);
    let compute = |pos: Pos2| -> f32 {
        let relative = axis(pos) - track_start - thumb_size * 0.5;
        (relative / thumb_travel.max(1.0)).clamp(0.0, 1.0) * max_scroll
    };

    if is_dragging && response.dragged() {
        if let Some(pos) = response.interact_pointer_pos() {
            *scroll = compute(pos);
        }
    }

    if response.clicked() {
        if let Some(pos) = response.interact_pointer_pos() {
            if track_rect.contains(pos) {
                *scroll = compute(pos);
            }
        }
    }
}

impl<'a> EditorWidget<'a> {
    /// Renders the vertical scrollbar and handles interaction.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn render_vertical_scrollbar(
        &mut self,
        _ui: &Ui,
        painter: &egui::Painter,
        response: &Response,
        full_rect: Rect,
        _text_area: &Rect,
        total_lines: usize,
        _visible_lines: usize,
        line_height: f32,
        hscroll_height: f32,
        pointer_pos: Option<Pos2>,
    ) {
        let track_rect = Rect::from_min_max(
            Pos2::new(full_rect.max.x - SCROLLBAR_WIDTH, full_rect.min.y),
            Pos2::new(full_rect.max.x, full_rect.max.y - hscroll_height),
        );
        painter.rect_filled(track_rect, 0.0, self.theme.scrollbar_track_color);

        let content_height = total_lines as f32 * line_height;
        let viewport_height = track_rect.height();
        if content_height <= 0.0 {
            return;
        }

        let thumb_height = (viewport_height * (viewport_height / content_height).min(1.0))
            .max(SCROLLBAR_MIN_THUMB);
        let max_scroll = (total_lines.saturating_sub(1)) as f32;
        let scroll_ratio = if max_scroll > 0.0 {
            self.doc.scroll_y / max_scroll
        } else {
            0.0
        };
        let thumb_travel = viewport_height - thumb_height;
        let thumb_y = track_rect.min.y + scroll_ratio * thumb_travel;

        let thumb_rect = Rect::from_min_size(
            Pos2::new(track_rect.min.x + 2.0, thumb_y),
            Vec2::new(SCROLLBAR_WIDTH - 4.0, thumb_height),
        );

        let is_dragging = self.doc.scrollbar_drag == ScrollbarDrag::Vertical;
        let color = thumb_color(
            self.theme,
            is_dragging,
            pointer_pos.is_some_and(|p| thumb_rect.contains(p)),
        );
        painter.rect_filled(thumb_rect, 3.0, color);

        handle_axis_scroll(
            &mut self.doc.scroll_y,
            response,
            is_dragging,
            track_rect,
            thumb_height,
            thumb_travel,
            max_scroll,
            |p| p.y,
        );
    }

    /// Renders the horizontal scrollbar and handles interaction.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn render_horizontal_scrollbar(
        &mut self,
        _ui: &Ui,
        painter: &egui::Painter,
        response: &Response,
        full_rect: Rect,
        text_area: &Rect,
        content_width: f32,
        vscroll_width: f32,
        pointer_pos: Option<Pos2>,
    ) {
        let track_rect = Rect::from_min_max(
            Pos2::new(full_rect.min.x, full_rect.max.y - SCROLLBAR_WIDTH),
            Pos2::new(full_rect.max.x - vscroll_width, full_rect.max.y),
        );
        painter.rect_filled(track_rect, 0.0, self.theme.scrollbar_track_color);

        let viewport_width = text_area.width();
        let track_width = track_rect.width();
        if content_width <= 0.0 || viewport_width <= 0.0 {
            return;
        }

        let thumb_width =
            (track_width * (viewport_width / content_width).min(1.0)).max(SCROLLBAR_MIN_THUMB);
        let max_scroll = (content_width - viewport_width).max(0.0);
        let scroll_ratio = if max_scroll > 0.0 {
            self.doc.scroll_x / max_scroll
        } else {
            0.0
        };
        let thumb_travel = track_width - thumb_width;
        let thumb_x = track_rect.min.x + scroll_ratio * thumb_travel;

        let thumb_rect = Rect::from_min_size(
            Pos2::new(thumb_x, track_rect.min.y + 2.0),
            Vec2::new(thumb_width, SCROLLBAR_WIDTH - 4.0),
        );

        let is_dragging = self.doc.scrollbar_drag == ScrollbarDrag::Horizontal;
        let color = thumb_color(
            self.theme,
            is_dragging,
            pointer_pos.is_some_and(|p| thumb_rect.contains(p)),
        );
        painter.rect_filled(thumb_rect, 3.0, color);

        handle_axis_scroll(
            &mut self.doc.scroll_x,
            response,
            is_dragging,
            track_rect,
            thumb_width,
            thumb_travel,
            max_scroll,
            |p| p.x,
        );
    }
}
