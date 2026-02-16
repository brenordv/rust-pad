//! Core editor widget: layout, rendering, scrolling, and mouse interaction.
//!
//! The `EditorWidget` struct ties together all editor subsystems (input, scrollbars,
//! special characters, word-wrap) and implements the main `show()` method that egui
//! calls each frame.

use egui::{
    text::LayoutJob, Color32, FontId, Pos2, Rect, Response, Sense, Stroke, TextFormat, Ui, Vec2,
};
use rust_pad_core::cursor::Position;
use rust_pad_core::document::{Document, ScrollbarDrag};

use super::painter::SyntaxHighlighter;
use super::render_cache::{get_render_cache, hash_str};
use super::theme::EditorTheme;
use super::wrap_map::WrapMap;

/// Extracts a line's content as a String, stripping the trailing newline if present.
///
/// Uses a single allocation instead of the previous pattern of
/// `l.to_string()` followed by `.trim_end_matches('\n').to_string()`.
fn line_content_string(slice: ropey::RopeSlice<'_>) -> String {
    let n = slice.len_chars();
    if n > 0 && slice.char(n - 1) == '\n' {
        slice.slice(..n - 1).to_string()
    } else {
        slice.to_string()
    }
}

/// Returns true if a `RopeSlice` ends with a newline character.
fn rope_slice_ends_with_newline(slice: ropey::RopeSlice<'_>) -> bool {
    let n = slice.len_chars();
    n > 0 && slice.char(n - 1) == '\n'
}

/// Scrollbar track width in logical pixels.
pub(crate) const SCROLLBAR_WIDTH: f32 = 14.0;
/// Minimum scrollbar thumb size in logical pixels.
pub(crate) const SCROLLBAR_MIN_THUMB: f32 = 20.0;

/// Left padding inside the text area so content doesn't touch the gutter edge.
const TEXT_LEFT_PADDING: f32 = 6.0;

/// Per-segment rendering parameters shared between wrapped and non-wrapped paths.
struct LineSegmentInfo<'a> {
    /// Y position of this line/segment on screen.
    line_y: f32,
    /// The visible text content (full line for non-wrapped, segment for wrapped).
    content: &'a str,
    /// Document char offset where this segment starts.
    segment_char_start: usize,
    /// Document char offset where this segment ends.
    segment_char_end: usize,
    /// Whether the logical line ends with a newline.
    line_has_newline: bool,
    /// Horizontal scroll offset (0.0 for wrapped mode).
    scroll_x: f32,
    /// When `Some(idx)`, enables galley caching for this line index.
    cache_line_idx: Option<usize>,
}

/// The custom editor widget that renders a Document.
pub struct EditorWidget<'a> {
    pub doc: &'a mut Document,
    pub theme: &'a EditorTheme,
    pub zoom_level: f32,
    pub highlighter: Option<&'a SyntaxHighlighter>,
    pub word_wrap: bool,
    pub show_special_chars: bool,
    pub show_line_numbers: bool,
    /// When true, the editor won't steal focus or process keyboard input.
    /// Set this when a dialog (Go to Line, Find/Replace, etc.) is open.
    pub dialog_open: bool,
    /// Zoom factor from Ctrl+scroll (1.0 = no change). Read by the app after `show()`.
    pub zoom_request: f32,
}

impl<'a> EditorWidget<'a> {
    pub fn new(
        doc: &'a mut Document,
        theme: &'a EditorTheme,
        zoom_level: f32,
        highlighter: Option<&'a SyntaxHighlighter>,
    ) -> Self {
        Self {
            doc,
            theme,
            zoom_level,
            highlighter,
            word_wrap: false,
            show_special_chars: false,
            show_line_numbers: true,
            dialog_open: false,
            zoom_request: 1.0,
        }
    }

    /// Returns the path used for syntax detection.
    ///
    /// Falls back to the document title when there is no file path,
    /// allowing untitled tabs with extensions (e.g. "Untitled.txt")
    /// to get appropriate syntax highlighting.
    pub(crate) fn syntax_path(&self) -> Option<std::path::PathBuf> {
        if self.doc.file_path.is_some() {
            return self.doc.file_path.clone();
        }
        let p = std::path::Path::new(&self.doc.title);
        if p.extension().is_some() {
            Some(std::path::PathBuf::from(&self.doc.title))
        } else {
            None
        }
    }

    /// Shows the editor widget and returns a response.
    pub fn show(&mut self, ui: &mut Ui) -> Response {
        let available = ui.available_size();
        let (response, painter) = ui.allocate_painter(available, Sense::click_and_drag());
        let rect = response.rect;

        let effective_font_size = self.theme.font_size * self.zoom_level;
        let font_id = FontId::monospace(effective_font_size);
        let line_height = effective_font_size * 1.4;
        let char_width = self.measure_char_width(ui, &font_id);

        let gutter_width = self.compute_gutter_width(ui, &font_id);
        let total_lines = self.doc.buffer.len_lines();
        let version_before_input = self.doc.content_version;

        // In wrap mode, assume vertical scrollbar is present for the initial build
        // to avoid building WrapMap twice. The error from an absent scrollbar is
        // only SCROLLBAR_WIDTH (14px) worth of wrap difference, which is acceptable.
        let wrap_map = if self.word_wrap {
            let exact_text_width = rect.width() - gutter_width - SCROLLBAR_WIDTH;
            let chars_per_line = (exact_text_width / char_width).floor().max(1.0) as usize;
            Some(WrapMap::build(self.doc, chars_per_line))
        } else {
            None
        };

        // Content dimensions depend on wrap mode.
        let total_visual_lines = wrap_map
            .as_ref()
            .map_or(total_lines, |w| w.total_visual_lines);
        let max_line_chars = if self.word_wrap {
            0
        } else {
            self.compute_max_line_chars(total_lines)
        };
        let content_width = max_line_chars as f32 * char_width;
        let content_height = total_visual_lines as f32 * line_height;

        // Determine which scrollbars are needed
        let inner_width = rect.width() - gutter_width;
        let inner_height = rect.height();
        let needs_vscroll = content_height > inner_height;
        let needs_hscroll = if self.word_wrap {
            false
        } else {
            content_width > (inner_width - if needs_vscroll { SCROLLBAR_WIDTH } else { 0.0 })
        };
        let needs_vscroll = if needs_hscroll {
            content_height > (inner_height - SCROLLBAR_WIDTH)
        } else {
            needs_vscroll
        };

        let vscroll_width = if needs_vscroll { SCROLLBAR_WIDTH } else { 0.0 };
        let hscroll_height = if needs_hscroll { SCROLLBAR_WIDTH } else { 0.0 };

        // Text area shrinks to accommodate scrollbars and left padding
        let text_area = Rect::from_min_max(
            Pos2::new(rect.min.x + gutter_width + TEXT_LEFT_PADDING, rect.min.y),
            Pos2::new(rect.max.x - vscroll_width, rect.max.y - hscroll_height),
        );
        let gutter_rect = Rect::from_min_max(
            rect.min,
            Pos2::new(rect.min.x + gutter_width, rect.max.y - hscroll_height),
        );

        // Background
        painter.rect_filled(rect, 0.0, self.theme.bg_color);
        if self.show_line_numbers {
            painter.rect_filled(gutter_rect, 0.0, self.theme.line_number_bg);

            // Gutter separator line
            painter.line_segment(
                [
                    Pos2::new(gutter_rect.max.x, rect.min.y),
                    Pos2::new(gutter_rect.max.x, rect.max.y - hscroll_height),
                ],
                Stroke::new(1.0, self.theme.gutter_separator_color),
            );
        }

        // Handle mouse-wheel scrolling
        let visible_lines = (text_area.height() / line_height).ceil() as usize;
        let max_scroll_y = (total_visual_lines.saturating_sub(1)) as f32;

        if response.hovered() {
            // Ctrl+scroll → zoom (egui converts Ctrl+scroll into zoom_delta,
            // removing it from smooth_scroll_delta)
            let zoom_delta = ui.input(|i| i.zoom_delta());
            if zoom_delta != 1.0 {
                self.zoom_request = zoom_delta;
            }

            let scroll_delta = ui.input(|i| i.smooth_scroll_delta);
            if scroll_delta.y != 0.0 {
                self.doc.scroll_y -= scroll_delta.y / line_height;
                self.doc.scroll_y = self.doc.scroll_y.clamp(0.0, max_scroll_y);
            }
            if !self.word_wrap && scroll_delta.x != 0.0 {
                let max_scroll_x = (content_width - text_area.width()).max(0.0);
                self.doc.scroll_x -= scroll_delta.x;
                self.doc.scroll_x = self.doc.scroll_x.clamp(0.0, max_scroll_x);
            }
        }

        // Compute scrollbar track rects so we can detect interactions early.
        let pointer_pos = ui.input(|i| i.pointer.interact_pos());
        let vscroll_track = if needs_vscroll {
            Some(Rect::from_min_max(
                Pos2::new(rect.max.x - SCROLLBAR_WIDTH, rect.min.y),
                Pos2::new(rect.max.x, rect.max.y - hscroll_height),
            ))
        } else {
            None
        };
        let hscroll_track = if needs_hscroll {
            Some(Rect::from_min_max(
                Pos2::new(rect.min.x, rect.max.y - SCROLLBAR_WIDTH),
                Pos2::new(rect.max.x - vscroll_width, rect.max.y),
            ))
        } else {
            None
        };

        // Track scrollbar drag state across frames.
        if response.drag_started() {
            if let Some(pos) = response.interact_pointer_pos() {
                if vscroll_track.is_some_and(|r| r.contains(pos)) {
                    self.doc.scrollbar_drag = ScrollbarDrag::Vertical;
                } else if hscroll_track.is_some_and(|r| r.contains(pos)) {
                    self.doc.scrollbar_drag = ScrollbarDrag::Horizontal;
                } else {
                    self.doc.scrollbar_drag = ScrollbarDrag::None;
                }
            }
        }
        if response.drag_stopped() {
            self.doc.scrollbar_drag = ScrollbarDrag::None;
        }

        let pointer_on_scrollbar = self.doc.scrollbar_drag != ScrollbarDrag::None
            || pointer_pos.is_some_and(|p| {
                vscroll_track.is_some_and(|r| r.contains(p))
                    || hscroll_track.is_some_and(|r| r.contains(p))
            });

        // Save cursor position before processing input to detect changes
        let cursor_pos_before = self.doc.cursor.position;

        // ── Process ALL input BEFORE rendering ──────────────────
        // This ensures the cursor and buffer are up-to-date when we render,
        // preventing a one-frame lag where the cursor appears at its old position.

        // Handle mouse clicks
        if !pointer_on_scrollbar && (response.clicked() || response.drag_started()) {
            self.doc.cursor_activity_time = ui.input(|i| i.time);
            if let Some(pos) = response.interact_pointer_pos() {
                if text_area.contains(pos) || gutter_rect.contains(pos) {
                    self.doc.clear_secondary_cursors();
                    if pos.x >= text_area.min.x {
                        let click_pos = self.screen_to_position(
                            pos,
                            text_area,
                            line_height,
                            char_width,
                            wrap_map.as_ref(),
                        );
                        if response.drag_started() && ui.input(|i| i.modifiers.shift) {
                            self.doc.cursor.start_selection();
                        } else if response.clicked() {
                            self.doc.cursor.clear_selection();
                        }
                        self.doc.cursor.move_to(click_pos, &self.doc.buffer);
                    }
                }
            }
            response.request_focus();
        }

        // Handle drag for selection
        if !pointer_on_scrollbar && response.dragged() {
            self.doc.cursor_activity_time = ui.input(|i| i.time);
            if let Some(pos) = response.interact_pointer_pos() {
                if pos.x >= text_area.min.x && pos.x <= text_area.max.x {
                    self.doc.cursor.start_selection();
                    let drag_pos = self.screen_to_position(
                        pos,
                        text_area,
                        line_height,
                        char_width,
                        wrap_map.as_ref(),
                    );
                    self.doc.cursor.move_to(drag_pos, &self.doc.buffer);
                }
            }
        }

        // Handle double-click to select word
        if response.double_clicked() {
            self.doc.cursor.select_word(&self.doc.buffer);
        }

        // Auto-focus the editor on first render so it's ready for typing,
        // but skip when a dialog is open so it doesn't steal focus from
        // dialog text fields.
        if !self.dialog_open && !response.has_focus() && !response.lost_focus() {
            response.request_focus();
        }

        // Handle keyboard input when focused and no dialog is open.
        // The EventFilter tells egui NOT to consume Tab or arrow keys for
        // focus navigation — we handle them ourselves for indent/dedent and
        // cursor movement.
        if !self.dialog_open && response.has_focus() {
            ui.memory_mut(|mem| {
                mem.set_focus_lock_filter(
                    response.id,
                    egui::EventFilter {
                        tab: true,
                        horizontal_arrows: true,
                        vertical_arrows: true,
                        escape: false,
                    },
                );
            });
            self.handle_keyboard_input(ui);
        }

        // Scroll to follow the cursor when it moved (before rendering so
        // the viewport is correct for this frame).
        // Also honor the `scroll_to_cursor` flag which is set by operations
        // that run outside the widget's input loop (e.g. paste, undo/redo
        // from global shortcuts).
        let cursor_moved =
            self.doc.cursor.position != cursor_pos_before || self.doc.scroll_to_cursor;
        self.doc.scroll_to_cursor = false;
        if cursor_moved {
            self.ensure_cursor_visible(
                visible_lines,
                text_area.width(),
                char_width,
                wrap_map.as_ref(),
            );
        }

        // ── Render with up-to-date state ────────────────────────

        // Recompute line count after input may have changed the buffer
        let total_lines = self.doc.buffer.len_lines();

        // Rebuild wrap map after input only if the buffer actually changed
        let wrap_map = if self.word_wrap && self.doc.content_version != version_before_input {
            let exact_text_width = rect.width() - gutter_width - vscroll_width;
            let chars_per_line = (exact_text_width / char_width).floor().max(1.0) as usize;
            Some(WrapMap::build(self.doc, chars_per_line))
        } else {
            wrap_map
        };
        let total_visual_lines = wrap_map
            .as_ref()
            .map_or(total_lines, |w| w.total_visual_lines);

        // Collect all selection ranges for highlighting (primary + secondary cursors)
        let all_selection_ranges = self.collect_selection_ranges();
        let cursor_lines = self.collect_cursor_lines();

        // Find occurrences of selected text for highlight-all
        let occurrence_ranges = self
            .doc
            .selected_text()
            .filter(|s| s.len() >= 2 && !s.contains('\n'))
            .map(|needle| self.find_occurrence_ranges(&needle, &all_selection_ranges))
            .unwrap_or_default();

        // Validate galley render cache
        {
            let cache = get_render_cache(&mut self.doc.render_cache);
            cache.validate(self.doc.content_version, effective_font_size);
        }

        if let Some(ref wm) = wrap_map {
            // ── Word-wrap rendering path ─────────────────────────
            let first_visual = self.doc.scroll_y as usize;
            let last_visual = (first_visual + visible_lines + 1).min(total_visual_lines);

            self.render_lines_wrapped(
                ui,
                &painter,
                &font_id,
                line_height,
                char_width,
                &text_area,
                &gutter_rect,
                first_visual,
                last_visual,
                &all_selection_ranges,
                &occurrence_ranges,
                &cursor_lines,
                wm,
            );
            self.render_cursors(
                ui,
                &painter,
                &response,
                &text_area,
                line_height,
                char_width,
                first_visual,
                last_visual,
                Some(wm),
            );
        } else {
            // ── Normal (non-wrapped) rendering path ──────────────
            let first_visible_line = self.doc.scroll_y as usize;
            let last_visible_line = (first_visible_line + visible_lines + 1).min(total_lines);

            self.render_lines(
                ui,
                &painter,
                &font_id,
                line_height,
                char_width,
                &text_area,
                &gutter_rect,
                first_visible_line,
                last_visible_line,
                &all_selection_ranges,
                &occurrence_ranges,
                &cursor_lines,
            );
            self.render_cursors(
                ui,
                &painter,
                &response,
                &text_area,
                line_height,
                char_width,
                first_visible_line,
                last_visible_line,
                None,
            );
        }

        // Request continuous repaint for cursor blink
        if response.has_focus() {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(500));
        }

        // Render scrollbars
        if needs_vscroll {
            self.render_vertical_scrollbar(
                ui,
                &painter,
                &response,
                rect,
                &text_area,
                total_visual_lines,
                visible_lines,
                line_height,
                hscroll_height,
                pointer_pos,
            );
        }
        if needs_hscroll {
            self.render_horizontal_scrollbar(
                ui,
                &painter,
                &response,
                rect,
                &text_area,
                content_width,
                vscroll_width,
                pointer_pos,
            );
        }
        if needs_vscroll && needs_hscroll {
            let corner = Rect::from_min_max(
                Pos2::new(rect.max.x - vscroll_width, rect.max.y - hscroll_height),
                rect.max,
            );
            painter.rect_filled(corner, 0.0, self.theme.scrollbar_track_color);
        }

        // Clamp scroll values to valid range
        let max_scroll_y = (total_visual_lines.saturating_sub(1)) as f32;
        self.doc.scroll_y = self.doc.scroll_y.clamp(0.0, max_scroll_y);
        if self.word_wrap {
            self.doc.scroll_x = 0.0;
        } else {
            let max_scroll_x = (content_width - text_area.width()).max(0.0);
            self.doc.scroll_x = self.doc.scroll_x.clamp(0.0, max_scroll_x);
        }

        response
    }

    /// Computes the maximum line length in characters across all lines.
    ///
    /// Results are cached on the document and reused when `content_version` is unchanged.
    fn compute_max_line_chars(&mut self, total_lines: usize) -> usize {
        let version = self.doc.content_version;
        if let Some((v, cached)) = self.doc.cached_max_line_chars {
            if v == version {
                return cached;
            }
        }
        let mut max_chars = 0usize;
        for line_idx in 0..total_lines {
            let len = self.doc.buffer.line_len_chars(line_idx).unwrap_or(0);
            if len > max_chars {
                max_chars = len;
            }
        }
        // Add a small margin so the cursor at end-of-line has room
        let result = max_chars + 4;
        self.doc.cached_max_line_chars = Some((version, result));
        result
    }

    /// Collects selection ranges from all cursors.
    fn collect_selection_ranges(&self) -> Vec<(usize, usize)> {
        let mut ranges = Vec::new();
        if let Ok(Some(range)) = self.doc.cursor.selection_char_range(&self.doc.buffer) {
            if range.0 != range.1 {
                ranges.push(range);
            }
        }
        for sc in &self.doc.secondary_cursors {
            if let Ok(Some(range)) = sc.selection_char_range(&self.doc.buffer) {
                if range.0 != range.1 {
                    ranges.push(range);
                }
            }
        }
        ranges
    }

    /// Collects all lines that have a cursor on them.
    fn collect_cursor_lines(&self) -> Vec<usize> {
        let mut lines = vec![self.doc.cursor.position.line];
        for sc in &self.doc.secondary_cursors {
            if !lines.contains(&sc.position.line) {
                lines.push(sc.position.line);
            }
        }
        lines
    }

    /// Finds all occurrences of `needle` in the buffer, returning char index ranges.
    ///
    /// Results are cached on the document, keyed by `(content_version, needle)`.
    /// Excludes ranges that overlap with any of the given selection ranges.
    fn find_occurrence_ranges(
        &mut self,
        needle: &str,
        selection_ranges: &[(usize, usize)],
    ) -> Vec<(usize, usize)> {
        let version = self.doc.content_version;

        // Check cache — the cached ranges include ALL occurrences (before selection filtering)
        let all_ranges = if let Some((v, ref cached_needle, ref ranges)) =
            self.doc.cached_occurrences
        {
            if v == version && cached_needle == needle {
                ranges.clone()
            } else {
                let ranges = self.compute_occurrence_ranges(needle);
                self.doc.cached_occurrences = Some((version, needle.to_string(), ranges.clone()));
                ranges
            }
        } else {
            let ranges = self.compute_occurrence_ranges(needle);
            self.doc.cached_occurrences = Some((version, needle.to_string(), ranges.clone()));
            ranges
        };

        // Filter out ranges that overlap any active selection
        all_ranges
            .into_iter()
            .filter(|&(char_start, char_end)| {
                !selection_ranges
                    .iter()
                    .any(|&(s, e)| char_start < e && char_end > s)
            })
            .collect()
    }

    /// Computes all occurrence char ranges of `needle` in the buffer.
    fn compute_occurrence_ranges(&self, needle: &str) -> Vec<(usize, usize)> {
        let text = self.doc.buffer.to_string();
        let needle_byte_len = needle.len();
        let needle_char_len = needle.chars().count();
        let mut ranges = Vec::new();
        let mut start = 0;
        while let Some(pos) = text[start..].find(needle) {
            let byte_start = start + pos;
            // O(log n) via ropey instead of O(n) text[..byte_start].chars().count()
            let char_start = self.doc.buffer.byte_to_char(byte_start).unwrap_or(0);
            let char_end = char_start + needle_char_len;
            ranges.push((char_start, char_end));
            start = byte_start + needle_byte_len;
        }
        ranges
    }

    /// Renders a single line segment: occurrence/selection highlights, text, and special chars.
    ///
    /// Shared between the wrapped and non-wrapped rendering paths.
    #[allow(clippy::too_many_arguments)]
    fn render_line_segment(
        &mut self,
        ui: &mut Ui,
        painter: &egui::Painter,
        font_id: &FontId,
        line_height: f32,
        char_width: f32,
        text_area: &Rect,
        info: &LineSegmentInfo<'_>,
        all_selection_ranges: &[(usize, usize)],
        occurrence_ranges: &[(usize, usize)],
    ) {
        let badge_char_width = char_width * 0.7;
        let has_badges = self.show_special_chars && Self::line_has_badges(info.content);
        let x_positions = if has_badges {
            Some(Self::compute_x_positions(
                info.content,
                char_width,
                badge_char_width,
            ))
        } else {
            None
        };

        let seg_len = info.segment_char_end - info.segment_char_start;

        // Occurrence highlights (drawn first so selections paint on top)
        for &(occ_start, occ_end) in occurrence_ranges {
            if occ_start < info.segment_char_end && occ_end > info.segment_char_start {
                let col_start = occ_start.saturating_sub(info.segment_char_start);
                let col_end = occ_end.saturating_sub(info.segment_char_start).min(seg_len);
                let x_start = text_area.min.x
                    + Self::col_to_x(x_positions.as_deref(), col_start, char_width)
                    - info.scroll_x;
                let x_end = text_area.min.x
                    + Self::col_to_x(x_positions.as_deref(), col_end, char_width)
                    - info.scroll_x;
                let occ_rect = Rect::from_min_max(
                    Pos2::new(x_start.max(text_area.min.x), info.line_y),
                    Pos2::new(x_end.min(text_area.max.x), info.line_y + line_height),
                );
                painter.rect_filled(occ_rect, 0.0, self.theme.occurrence_highlight_color);
            }
        }

        // Selection highlights
        for &(sel_start, sel_end) in all_selection_ranges {
            if sel_start < info.segment_char_end + 1 && sel_end > info.segment_char_start {
                let sel_col_start = sel_start.saturating_sub(info.segment_char_start);
                let sel_col_end = if sel_end < info.segment_char_end {
                    sel_end - info.segment_char_start
                } else {
                    seg_len + 1
                };

                let sel_x_start = text_area.min.x
                    + Self::col_to_x(x_positions.as_deref(), sel_col_start, char_width)
                    - info.scroll_x;
                let mut sel_x_end = text_area.min.x
                    + Self::col_to_x(x_positions.as_deref(), sel_col_end, char_width)
                    - info.scroll_x;

                // Extend selection to cover EOL badges when special chars are shown.
                if self.show_special_chars
                    && sel_end > info.segment_char_end
                    && info.line_has_newline
                {
                    let text_end_x = text_area.min.x
                        + Self::col_to_x(x_positions.as_deref(), seg_len, char_width)
                        - info.scroll_x;
                    let eol_w = Self::eol_badges_width(self.doc.line_ending, badge_char_width);
                    sel_x_end = sel_x_end.max(text_end_x + eol_w);
                }

                let sel_rect = Rect::from_min_max(
                    Pos2::new(sel_x_start.max(text_area.min.x), info.line_y),
                    Pos2::new(sel_x_end.min(text_area.max.x), info.line_y + line_height),
                );
                painter.rect_filled(sel_rect, 0.0, self.theme.selection_color);
            }
        }

        // Replace tabs with spaces for rendering.
        let render_content = info.content.replace('\t', " ");

        // Text rendering
        if has_badges {
            let char_colors = self.extract_char_colors(&render_content, font_id);
            let text_y = info.line_y + line_height * 0.15;
            let xp = x_positions.as_ref().unwrap();
            for (i, ch) in info.content.chars().enumerate() {
                let x = text_area.min.x + xp[i] - info.scroll_x;
                if x + char_width < text_area.min.x || x > text_area.max.x {
                    continue;
                }
                if matches!(ch, '\t' | '\u{00A0}' | '\u{200B}' | '\u{200C}' | '\u{200D}') {
                    continue;
                }
                let color = char_colors.get(i).copied().unwrap_or(self.theme.text_color);
                let ch_str = String::from(ch);
                painter.text(
                    Pos2::new(x, text_y),
                    egui::Align2::LEFT_TOP,
                    &ch_str,
                    font_id.clone(),
                    color,
                );
            }
        } else {
            let text_x = text_area.min.x - info.scroll_x;
            let text_pos = Pos2::new(text_x, info.line_y + line_height * 0.15);

            if let Some(highlighter) = &self.highlighter {
                if let Some(line_idx) = info.cache_line_idx {
                    // Galley-cached path for non-wrapped lines and first wrap rows
                    let content_hash = hash_str(&render_content);
                    let cached = {
                        let cache = get_render_cache(&mut self.doc.render_cache);
                        cache.get(line_idx, content_hash)
                    };
                    if let Some(galley) = cached {
                        painter.galley(text_pos, galley, Color32::WHITE);
                    } else {
                        let syntax = highlighter.detect_syntax(self.syntax_path().as_deref());
                        if let Some(mut hl) = highlighter.create_highlighter(syntax) {
                            let line_with_nl = format!("{render_content}\n");
                            let job =
                                highlighter.highlight_line(&line_with_nl, syntax, &mut hl, font_id);
                            let galley = ui.fonts_mut(|f| f.layout_job(job));
                            {
                                let cache = get_render_cache(&mut self.doc.render_cache);
                                cache.insert(line_idx, content_hash, galley.clone());
                            }
                            painter.galley(text_pos, galley, Color32::WHITE);
                        } else {
                            painter.text(
                                text_pos,
                                egui::Align2::LEFT_TOP,
                                &render_content,
                                font_id.clone(),
                                self.theme.text_color,
                            );
                        }
                    }
                } else {
                    // Non-cached highlighting (wrapped continuation rows)
                    let syntax = highlighter.detect_syntax(self.syntax_path().as_deref());
                    if let Some(mut hl) = highlighter.create_highlighter(syntax) {
                        let line_with_nl = format!("{render_content}\n");
                        let job =
                            highlighter.highlight_line(&line_with_nl, syntax, &mut hl, font_id);
                        let galley = ui.fonts_mut(|f| f.layout_job(job));
                        painter.galley(text_pos, galley, Color32::WHITE);
                    } else {
                        painter.text(
                            text_pos,
                            egui::Align2::LEFT_TOP,
                            &render_content,
                            font_id.clone(),
                            self.theme.text_color,
                        );
                    }
                }
            } else {
                painter.text(
                    text_pos,
                    egui::Align2::LEFT_TOP,
                    &render_content,
                    font_id.clone(),
                    self.theme.text_color,
                );
            }
        }

        // Special characters overlay
        if self.show_special_chars {
            let eol = if info.line_has_newline {
                Some(self.doc.line_ending)
            } else {
                None
            };
            self.render_special_chars_overlay(
                painter,
                font_id,
                info.line_y,
                line_height,
                char_width,
                text_area,
                info.content,
                info.scroll_x,
                eol,
                x_positions.as_deref(),
            );
        }
    }

    /// Renders the gutter for a line: current-line highlight, change tracking, line number.
    #[allow(clippy::too_many_arguments)]
    fn render_gutter_for_line(
        &self,
        painter: &egui::Painter,
        gutter_painter: &egui::Painter,
        font_id: &FontId,
        line_height: f32,
        text_area: &Rect,
        gutter_rect: &Rect,
        logical_line: usize,
        line_y: f32,
        cursor_lines: &[usize],
        show_line_number: bool,
        show_change_tracking: bool,
    ) {
        // Current line highlight
        if cursor_lines.contains(&logical_line) {
            let highlight_rect = Rect::from_min_size(
                Pos2::new(text_area.min.x, line_y),
                Vec2::new(text_area.width(), line_height),
            );
            painter.rect_filled(highlight_rect, 0.0, self.theme.current_line_highlight);
        }

        // Change tracking indicator
        if self.show_line_numbers
            && show_change_tracking
            && self.theme.show_change_tracking
            && logical_line < self.doc.line_changes.len()
        {
            use rust_pad_core::document::LineChangeState;
            let indicator_color = match self.doc.line_changes[logical_line] {
                LineChangeState::Modified => Some(self.theme.modified_line_color),
                LineChangeState::Saved => Some(self.theme.saved_line_color),
                LineChangeState::Unchanged => None,
            };
            if let Some(color) = indicator_color {
                let indicator_rect = Rect::from_min_size(
                    Pos2::new(gutter_rect.max.x - 3.0, line_y),
                    Vec2::new(3.0, line_height),
                );
                gutter_painter.rect_filled(indicator_rect, 0.0, color);
            }
        }

        // Line number
        if self.show_line_numbers && show_line_number {
            let line_num_text = format!("{}", logical_line + 1);
            let line_num_color = if cursor_lines.contains(&logical_line) {
                self.theme.text_color
            } else {
                self.theme.line_number_color
            };
            gutter_painter.text(
                Pos2::new(gutter_rect.max.x - 8.0, line_y + line_height * 0.15),
                egui::Align2::RIGHT_TOP,
                &line_num_text,
                font_id.clone(),
                line_num_color,
            );
        }
    }

    /// Renders visible lines: backgrounds, line numbers, selections, and text.
    #[allow(clippy::too_many_arguments)]
    fn render_lines(
        &mut self,
        ui: &mut Ui,
        painter: &egui::Painter,
        font_id: &FontId,
        line_height: f32,
        char_width: f32,
        text_area: &Rect,
        gutter_rect: &Rect,
        first_visible: usize,
        last_visible: usize,
        all_selection_ranges: &[(usize, usize)],
        occurrence_ranges: &[(usize, usize)],
        cursor_lines: &[usize],
    ) {
        // Clip text and selection rendering to the text area so horizontally
        // scrolled content never bleeds over the gutter / line numbers.
        let text_painter = painter.with_clip_rect(*text_area);
        let scroll_x = self.doc.scroll_x;

        for line_idx in first_visible..last_visible {
            let y_offset = (line_idx as f32 - self.doc.scroll_y) * line_height;
            let line_y = text_area.min.y + y_offset;

            if line_y + line_height < text_area.min.y || line_y > text_area.max.y {
                continue;
            }

            self.render_gutter_for_line(
                &text_painter,
                painter,
                font_id,
                line_height,
                text_area,
                gutter_rect,
                line_idx,
                line_y,
                cursor_lines,
                true,
                true,
            );

            let line_content = self
                .doc
                .buffer
                .line(line_idx)
                .map(line_content_string)
                .unwrap_or_default();

            let line_start_char = self.doc.buffer.line_to_char(line_idx).unwrap_or(0);
            let line_char_len = self.doc.buffer.line_len_chars(line_idx).unwrap_or(0);
            let line_has_newline = self
                .doc
                .buffer
                .line(line_idx)
                .map(rope_slice_ends_with_newline)
                .unwrap_or(false);

            let info = LineSegmentInfo {
                line_y,
                content: &line_content,
                segment_char_start: line_start_char,
                segment_char_end: line_start_char + line_char_len,
                line_has_newline,
                scroll_x,
                cache_line_idx: Some(line_idx),
            };

            self.render_line_segment(
                ui,
                &text_painter,
                font_id,
                line_height,
                char_width,
                text_area,
                &info,
                all_selection_ranges,
                occurrence_ranges,
            );
        }
    }

    /// Renders all cursors (primary + secondary) with blink and activity reset.
    /// When `wrap_map` is provided, uses wrapped visual coordinates.
    #[allow(clippy::too_many_arguments)]
    fn render_cursors(
        &self,
        ui: &Ui,
        painter: &egui::Painter,
        response: &Response,
        text_area: &Rect,
        line_height: f32,
        char_width: f32,
        first_visible: usize,
        last_visible: usize,
        wrap_map: Option<&WrapMap>,
    ) {
        let cursor_visible = {
            let time = ui.input(|i| i.time);
            let since_activity = time - self.doc.cursor_activity_time;
            if since_activity < 0.5 {
                true
            } else {
                ((time * 2.0) as u64).is_multiple_of(2)
            }
        };
        if cursor_visible && response.has_focus() {
            // Clip cursor rendering to the text area so cursors scrolled
            // out of view don't render over the gutter.
            let text_painter = painter.with_clip_rect(*text_area);
            let secondary_cursor_color = Color32::from_rgb(200, 200, 200);

            let all_cursors: Vec<(usize, usize, bool)> = std::iter::once((
                self.doc.cursor.position.line,
                self.doc.cursor.position.col,
                true,
            ))
            .chain(
                self.doc
                    .secondary_cursors
                    .iter()
                    .map(|sc| (sc.position.line, sc.position.col, false)),
            )
            .collect();

            for (cursor_line, cursor_col, is_primary) in all_cursors {
                let (visual_line, cursor_x) = if let Some(wm) = wrap_map {
                    let vl = wm.position_to_visual_line(cursor_line, cursor_col);
                    let vc = wm.position_to_visual_col(cursor_col);

                    // Badge-aware x for the wrap segment
                    let lc = self
                        .doc
                        .buffer
                        .line(cursor_line)
                        .map(line_content_string)
                        .unwrap_or_default();
                    let lchars: Vec<char> = lc.chars().collect();
                    let ws = (vl - wm.logical_to_visual(cursor_line)) * wm.chars_per_visual_line;
                    let we = (ws + wm.chars_per_visual_line).min(lchars.len());
                    let seg: String = lchars[ws..we].iter().collect();
                    let x = text_area.min.x + self.col_to_x_badge_aware(&seg, vc, char_width);
                    (vl, x)
                } else {
                    let lc = self
                        .doc
                        .buffer
                        .line(cursor_line)
                        .map(line_content_string)
                        .unwrap_or_default();
                    let x = text_area.min.x
                        + self.col_to_x_badge_aware(&lc, cursor_col, char_width)
                        - self.doc.scroll_x;
                    (cursor_line, x)
                };

                if visual_line >= first_visible
                    && visual_line < last_visible
                    && cursor_x >= text_area.min.x
                    && cursor_x <= text_area.max.x
                {
                    let y_offset = (visual_line as f32 - self.doc.scroll_y) * line_height;
                    let cursor_y = text_area.min.y + y_offset;
                    let color = if is_primary {
                        self.theme.cursor_color
                    } else {
                        secondary_cursor_color
                    };
                    text_painter.line_segment(
                        [
                            Pos2::new(cursor_x, cursor_y),
                            Pos2::new(cursor_x, cursor_y + line_height),
                        ],
                        Stroke::new(2.0, color),
                    );
                }
            }
        }
    }

    /// Renders visible lines in word-wrap mode.
    #[allow(clippy::too_many_arguments)]
    fn render_lines_wrapped(
        &mut self,
        ui: &mut Ui,
        painter: &egui::Painter,
        font_id: &FontId,
        line_height: f32,
        char_width: f32,
        text_area: &Rect,
        gutter_rect: &Rect,
        first_visual: usize,
        last_visual: usize,
        all_selection_ranges: &[(usize, usize)],
        occurrence_ranges: &[(usize, usize)],
        cursor_lines: &[usize],
        wm: &WrapMap,
    ) {
        for visual_line in first_visual..last_visual {
            let y_offset = (visual_line as f32 - self.doc.scroll_y) * line_height;
            let line_y = text_area.min.y + y_offset;

            if line_y + line_height < text_area.min.y || line_y > text_area.max.y {
                continue;
            }

            let (logical_line, wrap_row) = wm.visual_to_logical(visual_line);
            if logical_line >= self.doc.buffer.len_lines() {
                continue;
            }

            let is_first_row = wrap_row == 0;

            self.render_gutter_for_line(
                painter,
                painter,
                font_id,
                line_height,
                text_area,
                gutter_rect,
                logical_line,
                line_y,
                cursor_lines,
                is_first_row,
                is_first_row,
            );

            // Extract the portion of this logical line for this wrap row
            let line_content = self
                .doc
                .buffer
                .line(logical_line)
                .map(line_content_string)
                .unwrap_or_default();

            let total_chars = line_content.chars().count();
            let col_start = wrap_row * wm.chars_per_visual_line;
            let col_end = (col_start + wm.chars_per_visual_line).min(total_chars);
            let byte_start = line_content
                .char_indices()
                .nth(col_start)
                .map(|(i, _)| i)
                .unwrap_or(line_content.len());
            let byte_end = line_content
                .char_indices()
                .nth(col_end)
                .map(|(i, _)| i)
                .unwrap_or(line_content.len());
            let segment = &line_content[byte_start..byte_end];

            let line_start_char = self.doc.buffer.line_to_char(logical_line).unwrap_or(0);
            let is_last_segment = col_end == total_chars;
            let line_has_newline = is_last_segment
                && self
                    .doc
                    .buffer
                    .line(logical_line)
                    .map(rope_slice_ends_with_newline)
                    .unwrap_or(false);

            // Wrapped mode doesn't use galley caching
            let cache_line_idx = None;

            let info = LineSegmentInfo {
                line_y,
                content: segment,
                segment_char_start: line_start_char + col_start,
                segment_char_end: line_start_char + col_end,
                line_has_newline,
                scroll_x: 0.0,
                cache_line_idx,
            };

            self.render_line_segment(
                ui,
                painter,
                font_id,
                line_height,
                char_width,
                text_area,
                &info,
                all_selection_ranges,
                occurrence_ranges,
            );
        }
    }

    /// Resolves a relative x position to a column index, accounting for
    /// special-char badges when enabled.
    fn x_to_col_badge_aware(&self, segment: &str, relative_x: f32, char_width: f32) -> usize {
        if self.show_special_chars && Self::line_has_badges(segment) {
            let bcw = char_width * 0.7;
            let xp = Self::compute_x_positions(segment, char_width, bcw);
            Self::x_to_col(&xp, relative_x)
        } else {
            (relative_x / char_width).round().max(0.0) as usize
        }
    }

    /// Resolves a column index to an x offset, accounting for special-char
    /// badges when enabled.
    fn col_to_x_badge_aware(&self, line_content: &str, col: usize, char_width: f32) -> f32 {
        if self.show_special_chars && Self::line_has_badges(line_content) {
            let bcw = char_width * 0.7;
            let xp = Self::compute_x_positions(line_content, char_width, bcw);
            Self::col_to_x(Some(&xp), col, char_width)
        } else {
            col as f32 * char_width
        }
    }

    /// Converts screen position to document position.
    /// When a `WrapMap` is provided, uses wrapped coordinates.
    fn screen_to_position(
        &self,
        screen_pos: Pos2,
        text_area: Rect,
        line_height: f32,
        char_width: f32,
        wrap_map: Option<&WrapMap>,
    ) -> Position {
        let relative_y = screen_pos.y - text_area.min.y;

        if let Some(wm) = wrap_map {
            let visual_line = ((relative_y / line_height) + self.doc.scroll_y) as usize;
            let (logical_line, wrap_row) = wm.visual_to_logical(visual_line);
            let relative_x = screen_pos.x - text_area.min.x;

            let lc = self
                .doc
                .buffer
                .line(logical_line)
                .map(line_content_string)
                .unwrap_or_default();
            let lchars: Vec<char> = lc.chars().collect();
            let cs = wrap_row * wm.chars_per_visual_line;
            let ce = (cs + wm.chars_per_visual_line).min(lchars.len());
            let seg: String = lchars[cs..ce].iter().collect();
            let visual_col = self.x_to_col_badge_aware(&seg, relative_x, char_width);
            let logical_col = wrap_row * wm.chars_per_visual_line + visual_col;
            Position::new(logical_line, logical_col)
        } else {
            let line = ((relative_y / line_height) + self.doc.scroll_y) as usize;
            let relative_x = screen_pos.x - text_area.min.x + self.doc.scroll_x;
            let lc = self
                .doc
                .buffer
                .line(line)
                .map(line_content_string)
                .unwrap_or_default();
            let col = self.x_to_col_badge_aware(&lc, relative_x, char_width);
            Position::new(line, col)
        }
    }

    /// Ensures the cursor is visible by adjusting scroll.
    /// When a `WrapMap` is provided, uses wrapped visual coordinates.
    fn ensure_cursor_visible(
        &mut self,
        visible_lines: usize,
        text_width: f32,
        char_width: f32,
        wrap_map: Option<&WrapMap>,
    ) {
        let margin = 2.0;

        // Vertical scroll — in wrap mode use the visual line, otherwise use the logical line.
        let cursor_visual_y = wrap_map.map_or(self.doc.cursor.position.line as f32, |wm| {
            wm.position_to_visual_line(self.doc.cursor.position.line, self.doc.cursor.position.col)
                as f32
        });

        if cursor_visual_y < self.doc.scroll_y + margin {
            self.doc.scroll_y = (cursor_visual_y - margin).max(0.0);
        }
        if cursor_visual_y >= self.doc.scroll_y + visible_lines as f32 - margin {
            self.doc.scroll_y = cursor_visual_y - visible_lines as f32 + margin + 1.0;
        }

        // Horizontal scroll — only in non-wrap mode.
        if wrap_map.is_some() {
            self.doc.scroll_x = 0.0;
        } else {
            let lc = self
                .doc
                .buffer
                .line(self.doc.cursor.position.line)
                .map(line_content_string)
                .unwrap_or_default();
            let cursor_x = self.col_to_x_badge_aware(&lc, self.doc.cursor.position.col, char_width);
            let scroll_margin = char_width * 4.0;
            if cursor_x < self.doc.scroll_x + scroll_margin {
                self.doc.scroll_x = (cursor_x - scroll_margin).max(0.0);
            }
            if cursor_x > self.doc.scroll_x + text_width - scroll_margin {
                self.doc.scroll_x = cursor_x - text_width + scroll_margin;
            }
        }
    }

    /// Measures the width of a single character in the monospace font.
    fn measure_char_width(&self, ui: &Ui, font_id: &FontId) -> f32 {
        let mut job = LayoutJob::default();
        job.append(
            "M",
            0.0,
            TextFormat {
                font_id: font_id.clone(),
                ..Default::default()
            },
        );
        let galley = ui.fonts_mut(|f| f.layout_job(job));
        galley.rect.width()
    }

    /// Computes gutter width based on line count (0 when line numbers are hidden).
    fn compute_gutter_width(&self, ui: &Ui, font_id: &FontId) -> f32 {
        if !self.show_line_numbers {
            return 0.0;
        }
        let line_count = self.doc.buffer.len_lines();
        let digits = if line_count == 0 {
            1
        } else {
            (line_count as f64).log10().floor() as usize + 1
        }
        .max(3);
        let digit_width = self.measure_char_width(ui, font_id);
        (digits as f32 + 2.0) * digit_width + 8.0
    }
}
