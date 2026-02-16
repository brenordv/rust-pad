//! Word-wrap map for the editor widget.
//!
//! Precomputes how logical lines map to visual (wrapped) lines,
//! enabling efficient rendering and cursor placement in wrap mode.

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

    /// Returns the column offset within the visual line for a given logical column.
    pub fn position_to_visual_col(&self, col: usize) -> usize {
        col % self.chars_per_visual_line
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
