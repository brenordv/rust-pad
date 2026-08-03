//! Word-wrap map for the editor widget.
//!
//! Precomputes how logical lines map to visual (wrapped) lines,
//! so rendering and cursor placement in wrap mode don't recompute
//! the wrapping on every access.

use rust_pad_core::document::Document;

/// Precomputed word-wrap information for one frame.
///
/// For each logical line, stores the number of visual (wrapped) lines it occupies.
/// `visual_offset[i]` is the visual line index where logical line `i` starts.
pub(crate) struct WrapMap {
    /// Number of visual lines per logical line.
    visual_lines_per_logical: Vec<u16>,
    /// Cumulative visual line offset: `visual_offset[i]` = sum of visual_lines_per_logical[0..i].
    visual_offset: Vec<usize>,
    /// Total visual lines in the document.
    pub total_visual_lines: usize,
    /// Maximum number of characters that fit in one visual line.
    pub chars_per_visual_line: usize,
}

impl WrapMap {
    /// Builds a wrap map for the given document and available width.
    pub fn build(doc: &Document, chars_per_visual_line: usize) -> Self {
        let total_lines = doc.buffer.len_lines();
        let mut visual_lines_per_logical = Vec::with_capacity(total_lines);
        let mut visual_offset = Vec::with_capacity(total_lines);
        let mut cumulative = 0usize;

        let wrap_at = chars_per_visual_line.max(1);
        for line_idx in 0..total_lines {
            visual_offset.push(cumulative);
            let line_len = doc.buffer.line_len_chars(line_idx).unwrap_or(0);
            // Exclude trailing newline from wrapping calculation
            let content_len = if line_len > 0
                && doc
                    .buffer
                    .line(line_idx)
                    .map(|l| {
                        let n = l.len_chars();
                        n > 0 && l.char(n - 1) == '\n'
                    })
                    .unwrap_or(false)
            {
                line_len.saturating_sub(1)
            } else {
                line_len
            };
            let vlines = if content_len == 0 {
                1
            } else {
                content_len.div_ceil(wrap_at) as u16
            };
            visual_lines_per_logical.push(vlines);
            cumulative += vlines as usize;
        }

        Self {
            visual_lines_per_logical,
            visual_offset,
            total_visual_lines: cumulative,
            chars_per_visual_line: wrap_at,
        }
    }

    /// Returns the visual line index where a logical line starts.
    pub fn logical_to_visual(&self, logical_line: usize) -> usize {
        self.visual_offset
            .get(logical_line)
            .copied()
            .unwrap_or(self.total_visual_lines)
    }

    /// Returns the visual line offset for a given logical line + column.
    pub fn position_to_visual_line(&self, line: usize, col: usize) -> usize {
        let base = self.logical_to_visual(line);
        let wrap_offset = col / self.chars_per_visual_line;
        base + wrap_offset.min(self.visual_lines_for(line).saturating_sub(1))
    }

    /// Visual `(line, column)` for a caret at logical `(line, col)`.
    ///
    /// Row is [`logical_to_visual`](Self::logical_to_visual) plus the wrap row,
    /// and the column is the offset within that row. The one special case is a
    /// caret at the very end of a logical line whose length is an exact multiple
    /// of the wrap width: it pins to the right edge of the last visual row
    /// (column == wrap width) instead of wrapping back to column 0 of that row.
    pub fn caret_visual_position(&self, line: usize, col: usize) -> (usize, usize) {
        let rows = self.visual_lines_for(line); // always >= 1
        let base = self.logical_to_visual(line);
        let row = col / self.chars_per_visual_line;
        if row >= rows {
            // Only reachable when `col == rows * chars_per_visual_line`, i.e. the
            // caret is one past the last full row. Render it at the right edge of
            // the last row rather than column 0 of a row that does not exist.
            let last = rows - 1;
            (base + last, col - last * self.chars_per_visual_line)
        } else {
            (base + row, col % self.chars_per_visual_line)
        }
    }

    /// Returns how many visual lines a logical line occupies.
    pub fn visual_lines_for(&self, logical_line: usize) -> usize {
        self.visual_lines_per_logical
            .get(logical_line)
            .copied()
            .unwrap_or(1) as usize
    }

    /// Converts a visual line index back to (logical_line, wrap_row_within_line).
    pub fn visual_to_logical(&self, visual_line: usize) -> (usize, usize) {
        // Binary search for the logical line whose visual_offset <= visual_line
        let logical = match self.visual_offset.binary_search(&visual_line) {
            Ok(idx) => idx,
            Err(idx) => idx.saturating_sub(1),
        };
        let wrap_row = visual_line.saturating_sub(self.logical_to_visual(logical));
        (logical, wrap_row)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_pad_core::document::Document;

    fn doc_from(text: &str) -> Document {
        let mut doc = Document::new();
        if !text.is_empty() {
            doc.insert_text(text);
        }
        doc
    }

    // ── WrapMap::build ─────────────────────────────────────────────

    #[test]
    fn build_single_short_line() {
        let doc = doc_from("hello");
        let wm = WrapMap::build(&doc, 80);
        assert_eq!(wm.total_visual_lines, 1);
        assert_eq!(wm.visual_lines_for(0), 1);
    }

    #[test]
    fn build_wraps_long_line() {
        // 20 chars in a 10-char-wide viewport → 2 visual lines
        let doc = doc_from("abcdefghijklmnopqrst");
        let wm = WrapMap::build(&doc, 10);
        assert_eq!(wm.total_visual_lines, 2);
        assert_eq!(wm.visual_lines_for(0), 2);
    }

    #[test]
    fn build_empty_document() {
        let doc = doc_from("");
        let wm = WrapMap::build(&doc, 80);
        assert_eq!(wm.total_visual_lines, 1);
        assert_eq!(wm.visual_lines_for(0), 1);
    }

    #[test]
    fn build_multiple_lines() {
        // "ab\n" + "1234567890123456789\n" + "end"
        // line 0: "ab" (2 chars) → 1 visual line
        // line 1: "1234567890123456789" (19 chars) → ceil(19/10) = 2 visual lines
        // line 2: "end" (3 chars) → 1 visual line
        let doc = doc_from("ab\n1234567890123456789\nend");
        let wm = WrapMap::build(&doc, 10);
        assert_eq!(wm.visual_lines_for(0), 1);
        assert_eq!(wm.visual_lines_for(1), 2);
        assert_eq!(wm.visual_lines_for(2), 1);
        assert_eq!(wm.total_visual_lines, 4);
    }

    #[test]
    fn build_empty_lines() {
        let doc = doc_from("a\n\nb");
        let wm = WrapMap::build(&doc, 80);
        // 3 logical lines, each 1 visual line
        assert_eq!(wm.total_visual_lines, 3);
        assert_eq!(wm.visual_lines_for(1), 1); // empty line still gets 1 visual line
    }

    #[test]
    fn build_exact_fit() {
        // Exactly 10 chars in a 10-char viewport → 1 visual line (not 2)
        let doc = doc_from("1234567890");
        let wm = WrapMap::build(&doc, 10);
        assert_eq!(wm.visual_lines_for(0), 1);
        assert_eq!(wm.total_visual_lines, 1);
    }

    #[test]
    fn build_one_over_wraps() {
        // 11 chars in a 10-char viewport → 2 visual lines
        let doc = doc_from("12345678901");
        let wm = WrapMap::build(&doc, 10);
        assert_eq!(wm.visual_lines_for(0), 2);
        assert_eq!(wm.total_visual_lines, 2);
    }

    #[test]
    fn build_chars_per_visual_line_zero_clamped_to_one() {
        let doc = doc_from("abc");
        let wm = WrapMap::build(&doc, 0);
        // chars_per_visual_line is clamped to 1
        assert_eq!(wm.chars_per_visual_line, 1);
        assert_eq!(wm.visual_lines_for(0), 3);
    }

    // ── logical_to_visual ──────────────────────────────────────────

    #[test]
    fn logical_to_visual_basic() {
        // "ab\n" + "1234567890123456789\n" + "xy"
        // Line 0: "ab" (2 chars) → 1 visual line
        // Line 1: "1234567890123456789" (19 chars) → ceil(19/10) = 2 visual lines
        // Line 2: "xy" (2 chars) → 1 visual line
        let doc = doc_from("ab\n1234567890123456789\nxy");
        let wm = WrapMap::build(&doc, 10);
        assert_eq!(wm.logical_to_visual(0), 0);
        assert_eq!(wm.logical_to_visual(1), 1);
        assert_eq!(wm.logical_to_visual(2), 3);
    }

    #[test]
    fn logical_to_visual_out_of_bounds() {
        let doc = doc_from("abc");
        let wm = WrapMap::build(&doc, 80);
        assert_eq!(wm.logical_to_visual(999), wm.total_visual_lines);
    }

    // ── visual_to_logical ──────────────────────────────────────────

    #[test]
    fn visual_to_logical_no_wrap() {
        let doc = doc_from("a\nb\nc");
        let wm = WrapMap::build(&doc, 80);
        assert_eq!(wm.visual_to_logical(0), (0, 0));
        assert_eq!(wm.visual_to_logical(1), (1, 0));
        assert_eq!(wm.visual_to_logical(2), (2, 0));
    }

    #[test]
    fn visual_to_logical_with_wrap() {
        let doc = doc_from("abcdefghijklmnopqrst\nxy");
        let wm = WrapMap::build(&doc, 10);
        // Line 0: 20 chars → 2 visual lines (visual 0 and 1)
        // Line 1: 2 chars → visual line 2
        assert_eq!(wm.visual_to_logical(0), (0, 0));
        assert_eq!(wm.visual_to_logical(1), (0, 1));
        assert_eq!(wm.visual_to_logical(2), (1, 0));
    }

    // ── position_to_visual_line ────────────────────────────────────

    #[test]
    fn position_to_visual_line_no_wrap() {
        let doc = doc_from("abc\ndef");
        let wm = WrapMap::build(&doc, 80);
        assert_eq!(wm.position_to_visual_line(0, 2), 0);
        assert_eq!(wm.position_to_visual_line(1, 0), 1);
    }

    #[test]
    fn position_to_visual_line_with_wrap() {
        let doc = doc_from("12345678901234567890");
        let wm = WrapMap::build(&doc, 10);
        // Col 0-9 → visual line 0, col 10-19 → visual line 1
        assert_eq!(wm.position_to_visual_line(0, 0), 0);
        assert_eq!(wm.position_to_visual_line(0, 9), 0);
        assert_eq!(wm.position_to_visual_line(0, 10), 1);
        assert_eq!(wm.position_to_visual_line(0, 19), 1);
    }

    // ── caret_visual_position ──────────────────────────────────────

    #[test]
    fn caret_visual_position_non_boundary() {
        // Two visual rows (20 chars, width 10). Interior carets match the plain
        // row/col split.
        let doc = doc_from("12345678901234567890");
        let wm = WrapMap::build(&doc, 10);
        assert_eq!(wm.caret_visual_position(0, 0), (0, 0));
        assert_eq!(wm.caret_visual_position(0, 5), (0, 5));
        assert_eq!(wm.caret_visual_position(0, 15), (1, 5));
    }

    #[test]
    fn caret_visual_position_end_of_full_single_row_pins_right() {
        // Exactly 10 chars in a width-10 line: one full visual row. A caret at
        // the end must render at the right edge (col 10), not back at column 0.
        let doc = doc_from("1234567890");
        let wm = WrapMap::build(&doc, 10);
        assert_eq!(wm.visual_lines_for(0), 1);
        assert_eq!(wm.caret_visual_position(0, 10), (0, 10));
    }

    #[test]
    fn caret_visual_position_end_of_two_full_rows_pins_right() {
        // 20 chars, width 10: two full rows. A caret at the very end (col 20)
        // pins to the right edge of the second row (row 1, col 10).
        let doc = doc_from("12345678901234567890");
        let wm = WrapMap::build(&doc, 10);
        assert_eq!(wm.visual_lines_for(0), 2);
        assert_eq!(wm.caret_visual_position(0, 20), (1, 10));
    }

    #[test]
    fn caret_visual_position_mid_line_boundary_unchanged() {
        // A caret at col 10 with more content on the next row stays at column 0
        // of that next row (unchanged behavior; only the trailing edge moved).
        let doc = doc_from("12345678901234567890");
        let wm = WrapMap::build(&doc, 10);
        assert_eq!(wm.caret_visual_position(0, 10), (1, 0));
    }

    #[test]
    fn caret_visual_position_carries_base_offset() {
        // Line 0 wraps to 2 rows, so line 1's caret starts at visual line 2.
        let doc = doc_from("12345678901234567890\nxy");
        let wm = WrapMap::build(&doc, 10);
        assert_eq!(wm.caret_visual_position(1, 0), (2, 0));
        assert_eq!(wm.caret_visual_position(1, 2), (2, 2));
    }

    #[test]
    fn caret_visual_position_empty_line() {
        let doc = doc_from("");
        let wm = WrapMap::build(&doc, 10);
        assert_eq!(wm.caret_visual_position(0, 0), (0, 0));
    }

    // ── visual_lines_for out of bounds ─────────────────────────────

    #[test]
    fn visual_lines_for_out_of_bounds() {
        let doc = doc_from("abc");
        let wm = WrapMap::build(&doc, 80);
        assert_eq!(wm.visual_lines_for(999), 1); // default
    }
}
