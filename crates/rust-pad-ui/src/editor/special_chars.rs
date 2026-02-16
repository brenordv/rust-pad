//! Special character rendering for the editor widget.
//!
//! Renders invisible characters (NBSP, ZWSP, ZWNJ, ZWJ), whitespace indicators
//! (space dots, tab arrows), and line ending badges (LF, CR, CRLF) as overlays.

use egui::{Color32, FontId, Pos2, Rect, Vec2};
use rust_pad_core::encoding::LineEnding;

use super::widget::EditorWidget;

impl<'a> EditorWidget<'a> {
    /// Returns the total rendered width of a badge with the given label.
    pub(crate) fn badge_total_width(label: &str, badge_char_width: f32) -> f32 {
        let h_pad = badge_char_width * 0.4;
        label.len() as f32 * badge_char_width + h_pad * 2.0
    }

    /// Returns the display width of a character when special-chars mode is active.
    ///
    /// Badge characters return their badge width; all others return `char_width`.
    pub(crate) fn special_char_display_width(
        ch: char,
        char_width: f32,
        badge_char_width: f32,
    ) -> f32 {
        match ch {
            '\u{00A0}' => Self::badge_total_width("NBSP", badge_char_width),
            '\u{200B}' => Self::badge_total_width("ZWSP", badge_char_width),
            '\u{200C}' => Self::badge_total_width("ZWNJ", badge_char_width),
            '\u{200D}' => Self::badge_total_width("ZWJ", badge_char_width),
            _ => char_width,
        }
    }

    /// Computes cumulative x offsets for each character in a line.
    ///
    /// Returns a `Vec` with `len = chars_count + 1` (the last entry is the end-of-line x).
    pub(crate) fn compute_x_positions(
        line_content: &str,
        char_width: f32,
        badge_char_width: f32,
    ) -> Vec<f32> {
        let mut positions = Vec::with_capacity(line_content.chars().count() + 1);
        let mut x = 0.0f32;
        for ch in line_content.chars() {
            positions.push(x);
            x += Self::special_char_display_width(ch, char_width, badge_char_width);
        }
        positions.push(x);
        positions
    }

    /// Returns the total visual width of end-of-line badges for the given line ending.
    pub(crate) fn eol_badges_width(line_ending: LineEnding, badge_char_width: f32) -> f32 {
        match line_ending {
            LineEnding::Lf => Self::badge_total_width("LF", badge_char_width),
            LineEnding::Cr => Self::badge_total_width("CR", badge_char_width),
            LineEnding::CrLf => {
                Self::badge_total_width("CR", badge_char_width)
                    + 2.0
                    + Self::badge_total_width("LF", badge_char_width)
            }
        }
    }

    /// Returns `true` if the line contains characters that render as multi-cell badges.
    pub(crate) fn line_has_badges(line_content: &str) -> bool {
        line_content
            .chars()
            .any(|ch| matches!(ch, '\u{00A0}' | '\u{200B}' | '\u{200C}' | '\u{200D}'))
    }

    /// Looks up the x offset for a column from precomputed positions,
    /// falling back to `col * char_width` when no positions are provided.
    pub(crate) fn col_to_x(x_positions: Option<&[f32]>, col: usize, char_width: f32) -> f32 {
        if let Some(positions) = x_positions {
            if col < positions.len() {
                positions[col]
            } else {
                let last = positions.last().copied().unwrap_or(0.0);
                let beyond = col.saturating_sub(positions.len().saturating_sub(1));
                last + beyond as f32 * char_width
            }
        } else {
            col as f32 * char_width
        }
    }

    /// Finds the column index closest to `target_x` using precomputed positions.
    pub(crate) fn x_to_col(x_positions: &[f32], target_x: f32) -> usize {
        if x_positions.is_empty() {
            return 0;
        }
        let mut best_col = 0;
        let mut best_dist = f32::MAX;
        for (i, &x) in x_positions.iter().enumerate() {
            let dist = (x - target_x).abs();
            if dist < best_dist {
                best_dist = dist;
                best_col = i;
            }
        }
        best_col
    }

    /// Extracts per-character syntax highlight colors for a line.
    ///
    /// Falls back to the default text color when no highlighter is available.
    pub(crate) fn extract_char_colors(&self, line_content: &str, font_id: &FontId) -> Vec<Color32> {
        if let Some(highlighter) = &self.highlighter {
            let syntax = highlighter.detect_syntax(self.syntax_path().as_deref());
            if let Some(mut hl) = highlighter.create_highlighter(syntax) {
                let line_with_nl = format!("{line_content}\n");
                let job = highlighter.highlight_line(&line_with_nl, syntax, &mut hl, font_id);
                let mut colors = Vec::with_capacity(line_content.chars().count());
                for (byte_idx, _ch) in line_content.char_indices() {
                    let color = job
                        .sections
                        .iter()
                        .find(|s| s.byte_range.contains(&byte_idx))
                        .map(|s| s.format.color)
                        .unwrap_or(self.theme.text_color);
                    colors.push(color);
                }
                return colors;
            }
        }
        vec![self.theme.text_color; line_content.chars().count()]
    }

    /// Draws special character glyphs (spaces, tabs, line endings, etc.) as an overlay.
    ///
    /// Invisible characters (NBSP, ZWSP, ZWNJ, ZWJ) and line endings (CR, LF) are
    /// rendered as Notepad++-style labeled badges. Spaces and tabs use glyph overlays.
    ///
    /// When `x_positions` is `Some`, badge-aware cumulative x offsets are used so that
    /// badges do not overlap subsequent text.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn render_special_chars_overlay(
        &self,
        painter: &egui::Painter,
        font_id: &FontId,
        line_y: f32,
        line_height: f32,
        char_width: f32,
        text_area: &Rect,
        line_content: &str,
        scroll_x: f32,
        eol: Option<LineEnding>,
        x_positions: Option<&[f32]>,
    ) {
        let color = self.theme.special_char_color;
        let text_y = line_y + line_height * 0.15;
        let badge_font = FontId::monospace(font_id.size * 0.7);
        let badge_char_width = char_width * 0.7;

        for (i, ch) in line_content.chars().enumerate() {
            let x = text_area.min.x + Self::col_to_x(x_positions, i, char_width) - scroll_x;
            if x + char_width < text_area.min.x || x > text_area.max.x {
                continue;
            }
            match ch {
                ' ' => {
                    painter.text(
                        Pos2::new(x, text_y),
                        egui::Align2::LEFT_TOP,
                        "\u{00B7}",
                        font_id.clone(),
                        color,
                    );
                }
                '\t' => {
                    painter.text(
                        Pos2::new(x, text_y),
                        egui::Align2::LEFT_TOP,
                        "\u{2192}",
                        font_id.clone(),
                        color,
                    );
                }
                '\u{00A0}' => {
                    Self::render_badge(
                        painter,
                        "NBSP",
                        x,
                        line_y,
                        line_height,
                        badge_char_width,
                        &badge_font,
                        color,
                    );
                }
                '\u{200B}' => {
                    Self::render_badge(
                        painter,
                        "ZWSP",
                        x,
                        line_y,
                        line_height,
                        badge_char_width,
                        &badge_font,
                        color,
                    );
                }
                '\u{200C}' => {
                    Self::render_badge(
                        painter,
                        "ZWNJ",
                        x,
                        line_y,
                        line_height,
                        badge_char_width,
                        &badge_font,
                        color,
                    );
                }
                '\u{200D}' => {
                    Self::render_badge(
                        painter,
                        "ZWJ",
                        x,
                        line_y,
                        line_height,
                        badge_char_width,
                        &badge_font,
                        color,
                    );
                }
                _ => {}
            }
        }

        // Line ending badges
        if let Some(ending) = eol {
            let eol_x = text_area.min.x
                + Self::col_to_x(x_positions, line_content.chars().count(), char_width)
                - scroll_x;
            let badges: &[&str] = match ending {
                LineEnding::Lf => &["LF"],
                LineEnding::CrLf => &["CR", "LF"],
                LineEnding::Cr => &["CR"],
            };
            let mut bx = eol_x;
            for label in badges {
                if bx > text_area.max.x {
                    break;
                }
                let width = Self::render_badge(
                    painter,
                    label,
                    bx,
                    line_y,
                    line_height,
                    badge_char_width,
                    &badge_font,
                    color,
                );
                bx += width + 2.0;
            }
        }
    }

    /// Renders a single labeled badge at the given position.
    ///
    /// Returns the total width of the badge (for chaining multiple badges).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn render_badge(
        painter: &egui::Painter,
        label: &str,
        x: f32,
        line_y: f32,
        line_height: f32,
        badge_char_width: f32,
        badge_font: &FontId,
        color: Color32,
    ) -> f32 {
        let h_pad = badge_char_width * 0.4;
        let badge_height = line_height * 0.75;
        let badge_y = line_y + (line_height - badge_height) * 0.5;
        let label_width = label.len() as f32 * badge_char_width;
        let badge_width = label_width + h_pad * 2.0;
        let badge_rect =
            Rect::from_min_size(Pos2::new(x, badge_y), Vec2::new(badge_width, badge_height));
        let bg = Color32::from_rgba_premultiplied(
            color.r() / 2,
            color.g() / 2,
            color.b() / 2,
            color.a() / 2,
        );
        painter.rect_filled(badge_rect, 2.0, bg);
        painter.text(
            badge_rect.center(),
            egui::Align2::CENTER_CENTER,
            label,
            badge_font.clone(),
            color,
        );
        badge_width
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CHAR_W: f32 = 10.0;
    const BADGE_CHAR_W: f32 = 7.0;

    // ── badge_total_width ──────────────────────────────────────────────

    #[test]
    fn test_badge_total_width() {
        let h_pad = BADGE_CHAR_W * 0.4;
        assert!(
            (EditorWidget::badge_total_width("NBSP", BADGE_CHAR_W)
                - (4.0 * BADGE_CHAR_W + 2.0 * h_pad))
                .abs()
                < f32::EPSILON
        );
        assert!(
            (EditorWidget::badge_total_width("LF", BADGE_CHAR_W)
                - (2.0 * BADGE_CHAR_W + 2.0 * h_pad))
                .abs()
                < f32::EPSILON
        );
    }

    // ── special_char_display_width ─────────────────────────────────────

    #[test]
    fn test_special_char_display_width_badge_chars() {
        for ch in ['\u{00A0}', '\u{200B}', '\u{200C}', '\u{200D}'] {
            let w = EditorWidget::special_char_display_width(ch, CHAR_W, BADGE_CHAR_W);
            assert!(
                w > CHAR_W,
                "badge char {ch:?} should be wider than char_width"
            );
        }
    }

    #[test]
    fn test_special_char_display_width_normal_chars() {
        for ch in ['a', ' ', '\t', '0', '!'] {
            let w = EditorWidget::special_char_display_width(ch, CHAR_W, BADGE_CHAR_W);
            assert!(
                (w - CHAR_W).abs() < f32::EPSILON,
                "normal char {ch:?} should return char_width"
            );
        }
    }

    // ── compute_x_positions ────────────────────────────────────────────

    #[test]
    fn test_compute_x_positions_no_badges() {
        let positions = EditorWidget::compute_x_positions("abc", CHAR_W, BADGE_CHAR_W);
        assert_eq!(positions.len(), 4); // 3 chars + 1 end
        assert!((positions[0] - 0.0).abs() < f32::EPSILON);
        assert!((positions[1] - CHAR_W).abs() < f32::EPSILON);
        assert!((positions[2] - 2.0 * CHAR_W).abs() < f32::EPSILON);
        assert!((positions[3] - 3.0 * CHAR_W).abs() < f32::EPSILON);
    }

    #[test]
    fn test_compute_x_positions_with_badges() {
        // 'a' then NBSP then 'b'
        let input = "a\u{00A0}b";
        let positions = EditorWidget::compute_x_positions(input, CHAR_W, BADGE_CHAR_W);
        assert_eq!(positions.len(), 4);
        // First char: normal width
        assert!((positions[1] - CHAR_W).abs() < f32::EPSILON);
        // NBSP badge is wider than char_width
        let nbsp_width = EditorWidget::special_char_display_width('\u{00A0}', CHAR_W, BADGE_CHAR_W);
        assert!((positions[2] - (CHAR_W + nbsp_width)).abs() < f32::EPSILON);
    }

    // ── line_has_badges ────────────────────────────────────────────────

    #[test]
    fn test_line_has_badges_true() {
        assert!(EditorWidget::line_has_badges("hello\u{00A0}world"));
        assert!(EditorWidget::line_has_badges("a\u{200B}b"));
        assert!(EditorWidget::line_has_badges("\u{200C}"));
        assert!(EditorWidget::line_has_badges("\u{200D}"));
    }

    #[test]
    fn test_line_has_badges_false() {
        assert!(!EditorWidget::line_has_badges("hello world"));
        assert!(!EditorWidget::line_has_badges("tabs\there"));
        assert!(!EditorWidget::line_has_badges(""));
    }

    // ── col_to_x ──────────────────────────────────────────────────────

    #[test]
    fn test_col_to_x_with_positions() {
        let positions = vec![0.0, 10.0, 25.0, 35.0];
        assert!((EditorWidget::col_to_x(Some(&positions), 0, CHAR_W) - 0.0).abs() < f32::EPSILON);
        assert!((EditorWidget::col_to_x(Some(&positions), 2, CHAR_W) - 25.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_col_to_x_without_positions() {
        assert!((EditorWidget::col_to_x(None, 3, CHAR_W) - 30.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_col_to_x_beyond_bounds() {
        let positions = vec![0.0, 10.0, 20.0];
        // col=3 is one beyond the last index (2)
        let x = EditorWidget::col_to_x(Some(&positions), 3, CHAR_W);
        assert!((x - 30.0).abs() < f32::EPSILON);
    }

    // ── x_to_col ──────────────────────────────────────────────────────

    #[test]
    fn test_x_to_col_exact() {
        let positions = vec![0.0, 10.0, 20.0, 30.0];
        assert_eq!(EditorWidget::x_to_col(&positions, 10.0), 1);
        assert_eq!(EditorWidget::x_to_col(&positions, 0.0), 0);
        assert_eq!(EditorWidget::x_to_col(&positions, 30.0), 3);
    }

    #[test]
    fn test_x_to_col_between() {
        let positions = vec![0.0, 10.0, 20.0, 30.0];
        // 14.0 is closer to 10.0 (col 1) than 20.0 (col 2)
        assert_eq!(EditorWidget::x_to_col(&positions, 14.0), 1);
        // 16.0 is closer to 20.0 (col 2) than 10.0 (col 1)
        assert_eq!(EditorWidget::x_to_col(&positions, 16.0), 2);
    }

    #[test]
    fn test_x_to_col_empty() {
        assert_eq!(EditorWidget::x_to_col(&[], 5.0), 0);
    }

    // ── eol_badges_width ─────────────────────────────────────────────

    #[test]
    fn test_eol_badges_width_lf() {
        let w = EditorWidget::eol_badges_width(LineEnding::Lf, BADGE_CHAR_W);
        let expected = EditorWidget::badge_total_width("LF", BADGE_CHAR_W);
        assert!((w - expected).abs() < f32::EPSILON);
    }

    #[test]
    fn test_eol_badges_width_cr() {
        let w = EditorWidget::eol_badges_width(LineEnding::Cr, BADGE_CHAR_W);
        let expected = EditorWidget::badge_total_width("CR", BADGE_CHAR_W);
        assert!((w - expected).abs() < f32::EPSILON);
    }

    #[test]
    fn test_eol_badges_width_crlf() {
        let w = EditorWidget::eol_badges_width(LineEnding::CrLf, BADGE_CHAR_W);
        let cr = EditorWidget::badge_total_width("CR", BADGE_CHAR_W);
        let lf = EditorWidget::badge_total_width("LF", BADGE_CHAR_W);
        let expected = cr + 2.0 + lf;
        assert!((w - expected).abs() < f32::EPSILON);
    }

    #[test]
    fn test_eol_badges_width_wider_than_char_width() {
        // The eol badge should be wider than a single char_width,
        // confirming that the old selection extension (1*char_width)
        // was insufficient to cover the badge.
        for ending in [LineEnding::Lf, LineEnding::Cr, LineEnding::CrLf] {
            let w = EditorWidget::eol_badges_width(ending, BADGE_CHAR_W);
            assert!(
                w > CHAR_W,
                "eol badge for {ending:?} should be wider than char_width"
            );
        }
    }
}
