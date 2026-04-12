//! Pure pagination + line-wrapping logic for the print/export pipeline.
//!
//! This module has no egui, printpdf, or threading dependencies — it turns
//! a document string plus a [`PageLayout`] into a `Vec<Page>` that the
//! [`pdf`](super::pdf) module can render verbatim.
//!
//! Separating layout from rendering keeps the complex part of the feature
//! (pagination boundaries, line wrapping, tab expansion) unit-testable
//! without touching any PDF machinery.

/// Page geometry and typography settings for a single print job.
#[derive(Debug, Clone)]
pub struct PageLayout {
    /// Page width in points.
    pub page_width_pt: f32,
    /// Page height in points.
    pub page_height_pt: f32,
    /// Margin on all four sides, in points.
    pub margin_pt: f32,
    /// Body font size in points.
    pub font_size_pt: f32,
    /// Distance between successive text baselines in points.
    pub line_height_pt: f32,
    /// Vertical space reserved for the header (above the body).
    pub header_height_pt: f32,
    /// Vertical space reserved for the footer (below the body).
    pub footer_height_pt: f32,
    /// Horizontal advance of a single monospace glyph at `font_size_pt`,
    /// as a fraction of the font size. See
    /// [`font::CHAR_ADVANCE_RATIO`](super::font::CHAR_ADVANCE_RATIO).
    pub char_advance_ratio: f32,
    /// Whether line numbers are rendered in a left gutter.
    pub show_line_numbers: bool,
    /// Number of spaces a tab character expands to.
    pub tab_width: usize,
}

impl PageLayout {
    /// Returns the default A4 portrait layout used by the "Print..." and
    /// "Export as PDF..." menu entries.
    pub fn a4_default(show_line_numbers: bool) -> Self {
        // A4: 210mm × 297mm in points (1 mm = 2.8346 pt)
        Self {
            page_width_pt: 595.28,
            page_height_pt: 841.89,
            margin_pt: 36.0, // 0.5 inch
            font_size_pt: 9.0,
            line_height_pt: 11.0,
            header_height_pt: 28.0,
            footer_height_pt: 20.0,
            char_advance_ratio: super::font::CHAR_ADVANCE_RATIO,
            show_line_numbers,
            tab_width: 4,
        }
    }

    /// Width available for body content in points (between margins).
    pub fn content_width_pt(&self) -> f32 {
        (self.page_width_pt - 2.0 * self.margin_pt).max(0.0)
    }

    /// Height available for the body (excluding margins, header, footer).
    pub fn content_height_pt(&self) -> f32 {
        (self.page_height_pt - 2.0 * self.margin_pt - self.header_height_pt - self.footer_height_pt)
            .max(0.0)
    }

    /// Number of body lines that fit on a single page.
    pub fn lines_per_page(&self) -> usize {
        if self.line_height_pt <= 0.0 {
            return 1;
        }
        let n = (self.content_height_pt() / self.line_height_pt).floor() as usize;
        n.max(1)
    }

    /// Horizontal advance of one glyph in points.
    fn char_width_pt(&self) -> f32 {
        self.font_size_pt * self.char_advance_ratio
    }
}

/// A single visual line after wrapping. One source line may produce many
/// `LaidOutLine`s when it's longer than the page width.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaidOutLine {
    /// 1-based source line number from the original document. Used to
    /// render the gutter; continuation rows keep the same number.
    pub source_line: usize,
    /// True if this row is a soft-wrap continuation of the previous row
    /// (same `source_line`, no gutter number shown).
    pub is_continuation: bool,
    /// Text to render on this row. Tabs have already been expanded to
    /// spaces.
    pub text: String,
}

/// A laid-out page ready for rendering.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Page {
    pub lines: Vec<LaidOutLine>,
}

/// Computes the gutter width (in characters) required to display the
/// largest line number plus a fixed `" │ "` separator. Returns `0` when
/// line numbers are disabled.
///
/// Example: for 250 source lines the largest number is `250` (3 chars),
/// plus 3 chars of separator = 6.
pub fn gutter_width_chars(total_source_lines: usize, show_line_numbers: bool) -> usize {
    if !show_line_numbers {
        return 0;
    }
    let digits = digit_count(total_source_lines.max(1));
    // "<digits> │ " — the separator itself is 3 columns: space, bar, space.
    digits + 3
}

fn digit_count(n: usize) -> usize {
    if n == 0 {
        return 1;
    }
    let mut n = n;
    let mut d = 0;
    while n > 0 {
        d += 1;
        n /= 10;
    }
    d
}

/// Formats the gutter prefix for a given source line number. Continuation
/// rows pass `None` and get whitespace of the same width.
pub fn format_gutter(line_number: Option<usize>, gutter_width: usize) -> String {
    if gutter_width == 0 {
        return String::new();
    }
    // gutter_width = digits + 3 ("space bar space"), so the number field is
    // `gutter_width - 3` wide.
    let num_width = gutter_width.saturating_sub(3);
    match line_number {
        Some(n) => format!("{n:>num_width$} \u{2502} "),
        None => format!("{:num_width$} \u{2502} ", "", num_width = num_width),
    }
}

/// Expands tab characters in `s` to spaces, aligning to `tab_width`-column
/// stops.
///
/// This is column-aware: a tab at column 0 with `tab_width = 4` advances
/// to column 4, but a tab at column 2 also advances to column 4 (not 6).
fn expand_tabs(s: &str, tab_width: usize) -> String {
    if tab_width == 0 || !s.contains('\t') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut col = 0usize;
    for ch in s.chars() {
        if ch == '\t' {
            let spaces = tab_width - (col % tab_width);
            for _ in 0..spaces {
                out.push(' ');
            }
            col += spaces;
        } else {
            out.push(ch);
            col += 1;
        }
    }
    out
}

/// Splits a logical source line into visual rows no wider than
/// `body_chars_per_line`. Returns at least one row even for empty input,
/// so the source line is still rendered.
fn wrap_line(text: &str, body_chars_per_line: usize) -> Vec<String> {
    if body_chars_per_line == 0 {
        return vec![text.to_string()];
    }
    let chars: Vec<char> = text.chars().collect();
    if chars.is_empty() {
        return vec![String::new()];
    }
    let mut rows: Vec<String> = Vec::new();
    let mut idx = 0;
    while idx < chars.len() {
        let end = (idx + body_chars_per_line).min(chars.len());
        rows.push(chars[idx..end].iter().collect());
        idx = end;
    }
    rows
}

/// Paginates `document_text` according to `layout`.
///
/// An empty document produces a single empty page so the PDF is never
/// truly blank — the header and footer still render, which is useful
/// feedback that the "Print" action did something.
pub fn paginate(document_text: &str, layout: &PageLayout) -> Vec<Page> {
    // Split on `\n` and strip a trailing `\r` for CRLF safety. We do not
    // include an empty trailing row when the document ends with a newline,
    // matching the behavior of most editors' "print" output.
    let mut source_lines: Vec<&str> = document_text.split('\n').collect();
    if matches!(source_lines.last(), Some(&"")) && source_lines.len() > 1 {
        source_lines.pop();
    }
    let total_source_lines = source_lines.len().max(1);

    let gutter = gutter_width_chars(total_source_lines, layout.show_line_numbers);
    let char_width = layout.char_width_pt();
    let body_chars_per_line = if char_width > 0.0 {
        let total_chars = (layout.content_width_pt() / char_width).floor() as usize;
        total_chars.saturating_sub(gutter).max(1)
    } else {
        1
    };

    let lines_per_page = layout.lines_per_page();

    // Build all laid-out rows first, then chunk into pages.
    let mut rows: Vec<LaidOutLine> = Vec::new();

    if document_text.is_empty() {
        // Keep behavior predictable: one empty laid-out row, one page.
        rows.push(LaidOutLine {
            source_line: 1,
            is_continuation: false,
            text: String::new(),
        });
    } else {
        for (i, raw) in source_lines.iter().enumerate() {
            // Strip CR from CRLF line endings.
            let trimmed = raw.strip_suffix('\r').unwrap_or(raw);
            let expanded = expand_tabs(trimmed, layout.tab_width);
            let wrapped = wrap_line(&expanded, body_chars_per_line);
            for (row_idx, row_text) in wrapped.into_iter().enumerate() {
                rows.push(LaidOutLine {
                    source_line: i + 1,
                    is_continuation: row_idx > 0,
                    text: row_text,
                });
            }
        }
    }

    let mut pages = Vec::new();
    let mut page = Page::default();
    for row in rows {
        if page.lines.len() >= lines_per_page {
            pages.push(std::mem::take(&mut page));
        }
        page.lines.push(row);
    }
    if !page.lines.is_empty() || pages.is_empty() {
        pages.push(page);
    }
    pages
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout_80_cols_5_lines() -> PageLayout {
        // Synthetic layout: 80 char-wide body, exactly 5 body lines per page.
        // Using char_advance_ratio = 0.6 + font_size = 10 → char_width = 6 pt.
        // content_width = 80 * 6 = 480 → page_width = 480 + 2*margin.
        // content_height = 5 * 11 = 55 → page_height = 55 + 2*margin + hdr + ftr.
        let margin = 10.0;
        let hdr = 20.0;
        let ftr = 15.0;
        let line_h = 11.0;
        PageLayout {
            page_width_pt: 480.0 + 2.0 * margin,
            page_height_pt: 5.0 * line_h + 2.0 * margin + hdr + ftr,
            margin_pt: margin,
            font_size_pt: 10.0,
            line_height_pt: line_h,
            header_height_pt: hdr,
            footer_height_pt: ftr,
            char_advance_ratio: 0.6,
            show_line_numbers: false,
            tab_width: 4,
        }
    }

    #[test]
    fn empty_document_has_one_empty_page() {
        let pages = paginate("", &layout_80_cols_5_lines());
        assert_eq!(pages.len(), 1);
        assert_eq!(pages[0].lines.len(), 1);
        assert_eq!(pages[0].lines[0].text, "");
        assert_eq!(pages[0].lines[0].source_line, 1);
        assert!(!pages[0].lines[0].is_continuation);
    }

    #[test]
    fn single_short_line_fits_on_one_page() {
        let pages = paginate("hello", &layout_80_cols_5_lines());
        assert_eq!(pages.len(), 1);
        assert_eq!(pages[0].lines.len(), 1);
        assert_eq!(pages[0].lines[0].text, "hello");
        assert_eq!(pages[0].lines[0].source_line, 1);
    }

    #[test]
    fn exactly_full_page_stays_single_page() {
        let doc = (1..=5)
            .map(|n| format!("line {n}"))
            .collect::<Vec<_>>()
            .join("\n");
        let pages = paginate(&doc, &layout_80_cols_5_lines());
        assert_eq!(pages.len(), 1, "5 lines on a 5-line page = 1 page");
        assert_eq!(pages[0].lines.len(), 5);
    }

    #[test]
    fn one_line_over_page_spills_to_second_page() {
        let doc = (1..=6)
            .map(|n| format!("line {n}"))
            .collect::<Vec<_>>()
            .join("\n");
        let pages = paginate(&doc, &layout_80_cols_5_lines());
        assert_eq!(pages.len(), 2);
        assert_eq!(pages[0].lines.len(), 5);
        assert_eq!(pages[1].lines.len(), 1);
        assert_eq!(pages[1].lines[0].text, "line 6");
    }

    #[test]
    fn trailing_newline_does_not_produce_extra_row() {
        let pages = paginate("a\nb\n", &layout_80_cols_5_lines());
        // "a", "b" → 2 rows, not 3.
        let total_rows: usize = pages.iter().map(|p| p.lines.len()).sum();
        assert_eq!(total_rows, 2);
    }

    #[test]
    fn crlf_line_endings_are_stripped() {
        let pages = paginate("alpha\r\nbeta\r\n", &layout_80_cols_5_lines());
        assert_eq!(pages[0].lines[0].text, "alpha");
        assert_eq!(pages[0].lines[1].text, "beta");
    }

    #[test]
    fn very_long_line_wraps_preserving_line_number() {
        // 200 chars, 80-col body => 3 wrapped rows (80 + 80 + 40).
        let line = "x".repeat(200);
        let pages = paginate(&line, &layout_80_cols_5_lines());
        let rows: Vec<_> = pages.iter().flat_map(|p| p.lines.clone()).collect();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].text.len(), 80);
        assert_eq!(rows[1].text.len(), 80);
        assert_eq!(rows[2].text.len(), 40);
        for r in &rows {
            assert_eq!(r.source_line, 1);
        }
        assert!(!rows[0].is_continuation);
        assert!(rows[1].is_continuation);
        assert!(rows[2].is_continuation);
    }

    #[test]
    fn tabs_expand_to_aligned_stops() {
        assert_eq!(expand_tabs("a\tb", 4), "a   b"); // 'a' at col 0, tab → col 4
        assert_eq!(expand_tabs("ab\tc", 4), "ab  c"); // 'ab' at col 2, tab → col 4
        assert_eq!(expand_tabs("abcd\te", 4), "abcd    e"); // col 4, tab → col 8
        assert_eq!(expand_tabs("\t\t", 4), "        "); // two full tabs
        assert_eq!(expand_tabs("no tabs", 4), "no tabs");
    }

    #[test]
    fn tab_expansion_affects_wrap_width() {
        let mut layout = layout_80_cols_5_lines();
        layout.tab_width = 8;
        // "\t" + 73 x's = column 8 + 73 = 81 -> wraps (at 80)
        let doc = format!("\t{}", "x".repeat(73));
        let pages = paginate(&doc, &layout);
        let rows: Vec<_> = pages.iter().flat_map(|p| p.lines.clone()).collect();
        assert_eq!(rows.len(), 2, "line is 81 columns after tab expansion");
    }

    #[test]
    fn gutter_width_scales_with_line_count() {
        assert_eq!(gutter_width_chars(1, true), 4); // "1 │ "
        assert_eq!(gutter_width_chars(9, true), 4);
        assert_eq!(gutter_width_chars(10, true), 5); // "10 │ "
        assert_eq!(gutter_width_chars(99, true), 5);
        assert_eq!(gutter_width_chars(100, true), 6);
        assert_eq!(gutter_width_chars(9999, true), 7);
        assert_eq!(gutter_width_chars(0, true), 4); // max(1) -> 1 digit
        assert_eq!(gutter_width_chars(500, false), 0);
    }

    #[test]
    fn format_gutter_right_aligns_and_pads_continuations() {
        let w = gutter_width_chars(250, true); // 6: "NNN │ "
        assert_eq!(format_gutter(Some(1), w), "  1 \u{2502} ");
        assert_eq!(format_gutter(Some(250), w), "250 \u{2502} ");
        assert_eq!(format_gutter(None, w), "    \u{2502} ");
        assert_eq!(format_gutter(Some(1), 0), "");
    }

    #[test]
    fn pagination_with_line_numbers_reduces_body_width() {
        let mut layout = layout_80_cols_5_lines();
        layout.show_line_numbers = true;
        // 150 source lines → gutter "NNN │ " = 6 chars → body = 74 cols.
        let doc = (1..=150)
            .map(|n| format!("line-{n}"))
            .collect::<Vec<_>>()
            .join("\n");
        let pages = paginate(&doc, &layout);
        // Each short line fits without wrap → 150 rows on 5-lines-per-page = 30 pages.
        assert_eq!(pages.len(), 30);
    }

    #[test]
    fn source_line_numbers_are_monotonic_and_1_based() {
        let doc = "a\nb\nc";
        let pages = paginate(doc, &layout_80_cols_5_lines());
        let rows: Vec<_> = pages.iter().flat_map(|p| p.lines.clone()).collect();
        assert_eq!(rows[0].source_line, 1);
        assert_eq!(rows[1].source_line, 2);
        assert_eq!(rows[2].source_line, 3);
    }

    #[test]
    fn non_ascii_characters_are_counted_by_char_not_byte() {
        // Each é is 2 bytes but 1 char. Line should not wrap at 40 chars of é.
        let line = "é".repeat(40);
        let pages = paginate(&line, &layout_80_cols_5_lines());
        let rows: Vec<_> = pages.iter().flat_map(|p| p.lines.clone()).collect();
        assert_eq!(rows.len(), 1, "40 é's should fit on an 80-col line");
    }

    #[test]
    fn content_height_and_width_cannot_go_negative() {
        let layout = PageLayout {
            page_width_pt: 10.0,
            page_height_pt: 10.0,
            margin_pt: 100.0,
            font_size_pt: 9.0,
            line_height_pt: 11.0,
            header_height_pt: 0.0,
            footer_height_pt: 0.0,
            char_advance_ratio: 0.6,
            show_line_numbers: false,
            tab_width: 4,
        };
        assert_eq!(layout.content_width_pt(), 0.0);
        assert_eq!(layout.content_height_pt(), 0.0);
        assert!(layout.lines_per_page() >= 1);
    }

    #[test]
    fn paginate_degenerate_layout_still_produces_output() {
        // A layout where 0 chars fit per line still produces rows; we
        // guarantee at least 1 body char per line so no infinite loop.
        let layout = PageLayout {
            page_width_pt: 10.0,
            page_height_pt: 50.0,
            margin_pt: 4.0,
            font_size_pt: 9.0,
            line_height_pt: 11.0,
            header_height_pt: 0.0,
            footer_height_pt: 0.0,
            char_advance_ratio: 0.6,
            show_line_numbers: false,
            tab_width: 4,
        };
        let pages = paginate("hello", &layout);
        assert!(!pages.is_empty());
    }
}
