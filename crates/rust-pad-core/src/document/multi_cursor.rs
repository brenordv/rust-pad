//! Multi-cursor editing operations for documents.
//!
//! Provides text insertion, deletion, and selection operations that work
//! across multiple cursor positions simultaneously.

use crate::cursor::char_to_pos;

use super::Document;

/// A selection range captured against the current buffer.
///
/// `(start_char_idx, end_char_idx, source_index, is_primary)`. When the cursor
/// has no selection, `start == end == cursor.position` as char idx.
type SelectionRange = (usize, usize, usize, bool);

impl Document {
    /// Collects char indices for the primary and all secondary cursors.
    ///
    /// Returns `(char_idx, source_index, is_primary)` tuples (unsorted).
    fn collect_cursor_indices(&self) -> Vec<(usize, usize, bool)> {
        let mut indices = Vec::with_capacity(1 + self.secondary_cursors.len());
        if let Ok(idx) = self.cursor.to_char_index(&self.buffer) {
            indices.push((idx, 0, true));
        }
        for (i, sc) in self.secondary_cursors.iter().enumerate() {
            if let Ok(idx) = sc.to_char_index(&self.buffer) {
                indices.push((idx, i, false));
            }
        }
        indices
    }

    /// Captures `(start, end, src_idx, is_primary)` for every cursor against
    /// the current buffer. Cursors without a selection contribute a zero-width
    /// range `start == end` at their position.
    fn collect_selection_ranges(&self) -> Vec<SelectionRange> {
        let mut ranges = Vec::with_capacity(1 + self.secondary_cursors.len());

        let primary_idx = self.cursor.to_char_index(&self.buffer).unwrap_or(0);
        let (p_start, p_end) = match self.cursor.selection_char_range(&self.buffer) {
            Ok(Some((s, e))) => (s, e),
            _ => (primary_idx, primary_idx),
        };
        ranges.push((p_start, p_end, 0, true));

        for (i, sc) in self.secondary_cursors.iter().enumerate() {
            let sc_idx = sc.to_char_index(&self.buffer).unwrap_or(0);
            let (s, e) = match sc.selection_char_range(&self.buffer) {
                Ok(Some((s, e))) => (s, e),
                _ => (sc_idx, sc_idx),
            };
            ranges.push((s, e, i, false));
        }
        ranges
    }

    /// Updates a single cursor's position (primary or secondary).
    fn set_cursor_position(
        &mut self,
        src_idx: usize,
        is_primary: bool,
        new_pos: crate::cursor::Position,
        clear_selection: bool,
    ) {
        let cursor = if is_primary {
            &mut self.cursor
        } else {
            &mut self.secondary_cursors[src_idx]
        };
        cursor.position = new_pos;
        if clear_selection {
            cursor.clear_selection();
        }
        cursor.desired_col = None;
    }

    /// Finalizes a multi-cursor edit: merges overlapping cursors, syncs changes.
    fn finalize_multi_edit(&mut self, scroll_to_cursor: bool) {
        self.merge_overlapping_cursors();
        self.sync_line_changes();
        self.modified = true;
        if scroll_to_cursor {
            self.scroll_to_cursor = true;
        }
        self.bump_version();
    }

    /// Inserts text at all cursor positions (primary + secondary).
    pub fn insert_text_multi(&mut self, text: &str) {
        if self.secondary_cursors.is_empty() {
            self.insert_text(text);
            return;
        }

        // Capture ranges against the pre-mutation buffer. Any later math is
        // derived purely from these char indices, never from stale `Position`s.
        let mut ranges = self.collect_selection_ranges();
        let insert_len = text.chars().count();

        // Descending pass: delete each selection, then insert. Lower-index
        // positions stay valid because we only mutate from the end backwards.
        ranges.sort_by(|a, b| b.0.cmp(&a.0));
        for &(start, end, _, _) in &ranges {
            if end > start {
                let _ = self.buffer.remove(start, end);
            }
            let _ = self.buffer.insert(start, text);
        }

        // Ascending pass: each cursor lands `insert_len` past its (offset) start.
        ranges.sort_by_key(|&(start, _, _, _)| start);
        let mut offset: isize = 0;
        for &(start, end, src_idx, is_primary) in &ranges {
            let new_char_idx = (start as isize + offset + insert_len as isize).max(0) as usize;
            let new_pos = char_to_pos(&self.buffer, new_char_idx);
            self.set_cursor_position(src_idx, is_primary, new_pos, true);
            offset += insert_len as isize - (end as isize - start as isize);
        }

        self.finalize_multi_edit(true);
    }

    /// Inserts a different string at each cursor position (primary + secondary).
    ///
    /// `texts` must have exactly `1 + secondary_cursors.len()` elements:
    /// index 0 for the primary cursor, then one per secondary in order.
    pub fn insert_text_per_cursor(&mut self, texts: &[&str]) {
        let cursor_count = 1 + self.secondary_cursors.len();
        if texts.len() != cursor_count {
            if let Some(first) = texts.first() {
                self.insert_text_multi(first);
            }
            return;
        }

        if self.secondary_cursors.is_empty() {
            self.insert_text(texts[0]);
            return;
        }

        // Pair each cursor's selection range with its assigned text. The text
        // assignment follows `texts` order: index 0 → primary, then secondaries.
        let base_ranges = self.collect_selection_ranges();
        let mut ranges: Vec<(usize, usize, &str, usize, bool)> = base_ranges
            .into_iter()
            .map(|(s, e, src_idx, is_primary)| {
                let text_idx = if is_primary { 0 } else { src_idx + 1 };
                (s, e, texts[text_idx], src_idx, is_primary)
            })
            .collect();

        // Descending pass: delete + insert each per-cursor text.
        ranges.sort_by(|a, b| b.0.cmp(&a.0));
        for &(start, end, text, _, _) in &ranges {
            if end > start {
                let _ = self.buffer.remove(start, end);
            }
            let _ = self.buffer.insert(start, text);
        }

        // Ascending pass: place each cursor past its inserted text.
        ranges.sort_by_key(|&(start, _, _, _, _)| start);
        let mut offset: isize = 0;
        for &(start, end, text, src_idx, is_primary) in &ranges {
            let insert_len = text.chars().count();
            let new_char_idx = (start as isize + offset + insert_len as isize).max(0) as usize;
            let new_pos = char_to_pos(&self.buffer, new_char_idx);
            self.set_cursor_position(src_idx, is_primary, new_pos, true);
            offset += insert_len as isize - (end as isize - start as isize);
        }

        self.finalize_multi_edit(true);
    }

    /// Performs backspace at all cursor positions.
    pub fn backspace_multi(&mut self) {
        if self.secondary_cursors.is_empty() {
            self.backspace();
            return;
        }

        let has_selection = self.cursor.selection_anchor.is_some()
            || self
                .secondary_cursors
                .iter()
                .any(|c| c.selection_anchor.is_some());

        if has_selection {
            self.delete_selection_multi();
            return;
        }

        let mut indices = self.collect_cursor_indices();
        indices.sort_by(|a, b| b.0.cmp(&a.0));

        for &(char_idx, _, _) in &indices {
            if char_idx > 0 {
                let _ = self.buffer.remove(char_idx - 1, char_idx);
            }
        }

        // Update positions ascending with cumulative offset
        indices.sort_by_key(|&(idx, _, _)| idx);

        let mut deleted_count = 0usize;
        for &(original_idx, src_idx, is_primary) in &indices {
            if original_idx > 0 {
                let new_char_idx = original_idx - 1 - deleted_count;
                let new_pos = char_to_pos(&self.buffer, new_char_idx);
                self.set_cursor_position(src_idx, is_primary, new_pos, false);
                deleted_count += 1;
            } else {
                let new_pos = char_to_pos(&self.buffer, original_idx.saturating_sub(deleted_count));
                self.set_cursor_position(src_idx, is_primary, new_pos, false);
            }
        }

        self.finalize_multi_edit(false);
    }

    /// Deletes selections at all cursors (public alias).
    pub fn delete_selection_multi_public(&mut self) {
        self.delete_selection_multi();
    }

    /// Deletes selections at all cursors.
    fn delete_selection_multi(&mut self) {
        // Collect selection ranges, sort descending by start
        let mut ranges: Vec<(usize, usize, usize, bool)> = Vec::new();
        if let Ok(Some((s, e))) = self.cursor.selection_char_range(&self.buffer) {
            if s != e {
                ranges.push((s, e, 0, true));
            }
        }
        for (i, sc) in self.secondary_cursors.iter().enumerate() {
            if let Ok(Some((s, e))) = sc.selection_char_range(&self.buffer) {
                if s != e {
                    ranges.push((s, e, i, false));
                }
            }
        }
        ranges.sort_by(|a, b| b.0.cmp(&a.0));

        for &(start, end, _, _) in &ranges {
            let _ = self.buffer.remove(start, end);
        }

        // Update positions ascending
        ranges.sort_by_key(|&(start, _, _, _)| start);

        let mut offset = 0usize;
        for &(start, end, src_idx, is_primary) in &ranges {
            let new_pos = char_to_pos(&self.buffer, start.saturating_sub(offset));
            self.set_cursor_position(src_idx, is_primary, new_pos, true);
            offset += end - start;
        }

        // Also clear selection on cursors that had no selection
        self.cursor.clear_selection();
        for sc in &mut self.secondary_cursors {
            sc.clear_selection();
        }

        self.finalize_multi_edit(false);
    }

    /// Performs delete-forward at all cursor positions.
    pub fn delete_forward_multi(&mut self) {
        if self.secondary_cursors.is_empty() {
            self.delete_forward();
            return;
        }

        let has_selection = self.cursor.selection_anchor.is_some()
            || self
                .secondary_cursors
                .iter()
                .any(|c| c.selection_anchor.is_some());

        if has_selection {
            self.delete_selection_multi();
            return;
        }

        let total = self.buffer.len_chars();
        let mut indices = self.collect_cursor_indices();
        indices.sort_by(|a, b| b.0.cmp(&a.0));

        for &(char_idx, _, _) in &indices {
            if char_idx < total {
                let _ = self.buffer.remove(char_idx, char_idx + 1);
            }
        }

        // Update positions ascending
        indices.sort_by_key(|&(idx, _, _)| idx);

        let mut deleted_count = 0usize;
        for &(original_idx, src_idx, is_primary) in &indices {
            let new_char_idx = original_idx.saturating_sub(deleted_count);
            let new_pos = char_to_pos(&self.buffer, new_char_idx);
            self.set_cursor_position(src_idx, is_primary, new_pos, false);
            if original_idx < total {
                deleted_count += 1;
            }
        }

        self.finalize_multi_edit(false);
    }

    /// Inserts a newline at all cursor positions, inheriting each cursor's
    /// line leading whitespace (auto-indent).
    pub fn insert_newline_multi(&mut self) {
        if self.secondary_cursors.is_empty() {
            self.insert_newline();
            return;
        }

        // Build a per-cursor newline+indent string.
        // Index 0 = primary cursor, then one per secondary in order.
        let mut texts: Vec<String> = Vec::with_capacity(1 + self.secondary_cursors.len());
        let primary_indent = self
            .buffer
            .leading_whitespace(self.cursor.position.line)
            .unwrap_or_default();
        texts.push(format!("\n{primary_indent}"));

        for sc in &self.secondary_cursors {
            let indent = self
                .buffer
                .leading_whitespace(sc.position.line)
                .unwrap_or_default();
            texts.push(format!("\n{indent}"));
        }

        let refs: Vec<&str> = texts.iter().map(String::as_str).collect();
        self.insert_text_per_cursor(&refs);
    }

    /// Returns selected text from all cursors, joined by newlines.
    pub fn selected_text_multi(&self) -> Option<String> {
        if self.secondary_cursors.is_empty() {
            return self.selected_text();
        }

        let mut texts: Vec<String> = Vec::new();
        if let Some(text) = self.selected_text() {
            texts.push(text);
        }
        for sc in &self.secondary_cursors {
            if let Ok(Some((start, end))) = sc.selection_char_range(&self.buffer) {
                if start != end {
                    if let Ok(slice) = self.buffer.slice(start, end) {
                        texts.push(slice.to_string());
                    }
                }
            }
        }

        if texts.is_empty() {
            None
        } else {
            Some(texts.join("\n"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cursor::{Cursor, Position};

    /// Helper: creates a Document with text and cursor at (0,0).
    fn doc_with(text: &str) -> Document {
        let mut doc = Document::new();
        if !text.is_empty() {
            doc.insert_text(text);
        }
        doc.cursor.position = Position::new(0, 0);
        doc.cursor.clear_selection();
        doc
    }

    /// Helper: adds a secondary cursor at the given position.
    fn add_cursor(doc: &mut Document, line: usize, col: usize) {
        let mut sc = Cursor::new();
        sc.position = Position::new(line, col);
        doc.secondary_cursors.push(sc);
    }

    // ── insert_text_multi ─────────────────────────────────────────

    #[test]
    fn insert_multi_single_cursor_delegates() {
        let mut doc = doc_with("hello");
        doc.cursor.position = Position::new(0, 5);
        // No secondary cursors — should behave like insert_text
        doc.insert_text_multi("!");
        assert_eq!(doc.buffer.to_string(), "hello!");
    }

    #[test]
    fn insert_multi_replaces_selections() {
        let mut doc = doc_with("hello world");
        // Select "hello" with primary cursor
        doc.cursor.position = Position::new(0, 5);
        doc.cursor.selection_anchor = Some(Position::new(0, 0));
        // Select "world" with secondary
        let mut sc = Cursor::new();
        sc.position = Position::new(0, 11);
        sc.selection_anchor = Some(Position::new(0, 6));
        doc.secondary_cursors.push(sc);

        doc.insert_text_multi("X");
        assert_eq!(doc.buffer.to_string(), "X X");
    }

    #[test]
    fn insert_multi_multichar_text() {
        let mut doc = doc_with("ab\ncd");
        doc.cursor.position = Position::new(0, 1);
        add_cursor(&mut doc, 1, 1);

        doc.insert_text_multi("XX");
        assert_eq!(doc.buffer.to_string(), "aXXb\ncXXd");
    }

    // ── insert_text_per_cursor ────────────────────────────────────

    #[test]
    fn per_cursor_single_cursor_delegates() {
        let mut doc = doc_with("hello");
        doc.cursor.position = Position::new(0, 5);
        // No secondary cursors
        doc.insert_text_per_cursor(&["!"]);
        assert_eq!(doc.buffer.to_string(), "hello!");
    }

    #[test]
    fn per_cursor_empty_texts_array() {
        let mut doc = doc_with("hello");
        doc.cursor.position = Position::new(0, 5);
        add_cursor(&mut doc, 0, 0);
        // Empty texts — fallback to insert_text_multi with first (none available)
        doc.insert_text_per_cursor(&[]);
        // No crash, text should be unchanged
        assert_eq!(doc.buffer.to_string(), "hello");
    }

    #[test]
    fn per_cursor_with_selections_replaces() {
        let mut doc = doc_with("AAA BBB");
        // Select "AAA" with primary
        doc.cursor.position = Position::new(0, 3);
        doc.cursor.selection_anchor = Some(Position::new(0, 0));
        // Select "BBB" with secondary
        let mut sc = Cursor::new();
        sc.position = Position::new(0, 7);
        sc.selection_anchor = Some(Position::new(0, 4));
        doc.secondary_cursors.push(sc);

        doc.insert_text_per_cursor(&["X", "Y"]);
        assert_eq!(doc.buffer.to_string(), "X Y");
    }

    // ── same-line multi-cursor selection replace (regression) ────────

    /// Helper: adds a secondary cursor with a selection spanning [anchor_col, head_col].
    fn add_cursor_with_selection(
        doc: &mut Document,
        line: usize,
        anchor_col: usize,
        head_col: usize,
    ) {
        let mut sc = Cursor::new();
        sc.selection_anchor = Some(Position::new(line, anchor_col));
        sc.position = Position::new(line, head_col);
        doc.secondary_cursors.push(sc);
    }

    #[test]
    fn insert_multi_same_line_replaces_all_selections() {
        // The exact bug report: four `photo` selections on one line,
        // each replaced with `video`. Same length keeps offsets clean.
        let mut doc = doc_with(
            "- Sub categories: photo-of-food, photo-of-animals, photo-of-people, photo-of-places",
        );
        // Primary selects the first "photo" at columns 18..23.
        doc.cursor.selection_anchor = Some(Position::new(0, 18));
        doc.cursor.position = Position::new(0, 23);
        add_cursor_with_selection(&mut doc, 0, 33, 38);
        add_cursor_with_selection(&mut doc, 0, 51, 56);
        add_cursor_with_selection(&mut doc, 0, 68, 73);

        doc.insert_text_multi("video");
        assert_eq!(
            doc.buffer.to_string(),
            "- Sub categories: video-of-food, video-of-animals, video-of-people, video-of-places",
        );
    }

    #[test]
    fn insert_multi_same_line_with_grow_text() {
        // Each selection replaces 1 char with 3 chars (net growth per cursor).
        // Verifies the ascending-pass running offset stays correct when
        // insert_len > deleted_len.
        let mut doc = doc_with("aa..aa..aa");
        // Three single-char "a" selections in document order.
        doc.cursor.selection_anchor = Some(Position::new(0, 0));
        doc.cursor.position = Position::new(0, 1);
        add_cursor_with_selection(&mut doc, 0, 4, 5);
        add_cursor_with_selection(&mut doc, 0, 8, 9);

        doc.insert_text_multi("XYZ");
        // Each "a" → "XYZ"; the second `a` of each pair is untouched.
        assert_eq!(doc.buffer.to_string(), "XYZa..XYZa..XYZa");
    }

    #[test]
    fn insert_multi_same_line_with_shrink_text() {
        // Each selection replaces 4 chars with 1 char (net shrink per cursor).
        // Verifies the ascending-pass running offset stays correct when
        // insert_len < deleted_len.
        let mut doc = doc_with("WORD-WORD-WORD");
        doc.cursor.selection_anchor = Some(Position::new(0, 0));
        doc.cursor.position = Position::new(0, 4);
        add_cursor_with_selection(&mut doc, 0, 5, 9);
        add_cursor_with_selection(&mut doc, 0, 10, 14);

        doc.insert_text_multi("x");
        assert_eq!(doc.buffer.to_string(), "x-x-x");
    }

    #[test]
    fn per_cursor_same_line_replaces_all_selections() {
        // Same shape as the photo→video bug but routed through
        // insert_text_per_cursor (the code path used by Enter for auto-indent
        // and by multi-cursor paste).
        let mut doc = doc_with("aa.bb.cc.dd");
        doc.cursor.selection_anchor = Some(Position::new(0, 0));
        doc.cursor.position = Position::new(0, 2);
        add_cursor_with_selection(&mut doc, 0, 3, 5);
        add_cursor_with_selection(&mut doc, 0, 6, 8);
        add_cursor_with_selection(&mut doc, 0, 9, 11);

        doc.insert_text_per_cursor(&["W", "X", "Y", "Z"]);
        assert_eq!(doc.buffer.to_string(), "W.X.Y.Z");
    }

    // ── backspace_multi ───────────────────────────────────────────

    #[test]
    fn backspace_multi_single_cursor_delegates() {
        let mut doc = doc_with("abc");
        doc.cursor.position = Position::new(0, 3);
        doc.backspace_multi();
        assert_eq!(doc.buffer.to_string(), "ab");
    }

    #[test]
    fn backspace_multi_at_start_noop() {
        let mut doc = doc_with("hello");
        doc.cursor.position = Position::new(0, 0);
        add_cursor(&mut doc, 0, 0);
        // Both cursors at position 0 — nothing to delete
        doc.backspace_multi();
        assert_eq!(doc.buffer.to_string(), "hello");
    }

    #[test]
    fn backspace_multi_with_selection_deletes_selection() {
        let mut doc = doc_with("hello world");
        // Select "hello" with primary
        doc.cursor.position = Position::new(0, 5);
        doc.cursor.selection_anchor = Some(Position::new(0, 0));
        // No secondary cursors, but has_selection triggers delete_selection_multi
        add_cursor(&mut doc, 0, 11);

        doc.backspace_multi();
        // The selection on primary should be deleted
        assert_eq!(doc.buffer.to_string(), " world");
    }

    // ── delete_forward_multi ──────────────────────────────────────

    #[test]
    fn delete_forward_multi_single_cursor_delegates() {
        let mut doc = doc_with("abc");
        doc.cursor.position = Position::new(0, 0);
        doc.delete_forward_multi();
        assert_eq!(doc.buffer.to_string(), "bc");
    }

    #[test]
    fn delete_forward_multi_at_end_noop() {
        let mut doc = doc_with("ab");
        doc.cursor.position = Position::new(0, 2);
        add_cursor(&mut doc, 0, 2);
        doc.delete_forward_multi();
        assert_eq!(doc.buffer.to_string(), "ab");
    }

    #[test]
    fn delete_forward_multi_with_selection_deletes_selection() {
        let mut doc = doc_with("hello world");
        // Select "world" with primary
        doc.cursor.position = Position::new(0, 11);
        doc.cursor.selection_anchor = Some(Position::new(0, 6));
        add_cursor(&mut doc, 0, 0);

        doc.delete_forward_multi();
        assert_eq!(doc.buffer.to_string(), "hello ");
    }

    // ── delete_selection_multi_public ──────────────────────────────

    #[test]
    fn delete_selection_multi_public_no_selection_noop() {
        let mut doc = doc_with("hello");
        doc.cursor.position = Position::new(0, 3);
        add_cursor(&mut doc, 0, 1);
        // No selections on any cursor
        doc.delete_selection_multi_public();
        assert_eq!(doc.buffer.to_string(), "hello");
    }

    #[test]
    fn delete_selection_multi_public_multiple_selections() {
        let mut doc = doc_with("aabbcc");
        // Select "aa" with primary
        doc.cursor.position = Position::new(0, 2);
        doc.cursor.selection_anchor = Some(Position::new(0, 0));
        // Select "cc" with secondary
        let mut sc = Cursor::new();
        sc.position = Position::new(0, 6);
        sc.selection_anchor = Some(Position::new(0, 4));
        doc.secondary_cursors.push(sc);

        doc.delete_selection_multi_public();
        assert_eq!(doc.buffer.to_string(), "bb");
    }

    // ── insert_newline_multi ──────────────────────────────────────

    #[test]
    fn insert_newline_multi_single_cursor_delegates() {
        let mut doc = doc_with("ab");
        doc.cursor.position = Position::new(0, 1);
        doc.insert_newline_multi();
        assert_eq!(doc.buffer.to_string(), "a\nb");
    }

    #[test]
    fn insert_newline_multi_inherits_indent_per_cursor() {
        // Line 0: "    aa", Line 1: "  bb"
        let mut doc = doc_with("    aa\n  bb");
        doc.cursor.position = Position::new(0, 6); // end of "    aa"
        add_cursor(&mut doc, 1, 4); // end of "  bb"

        doc.insert_newline_multi();
        // Primary was on line 0 (4-space indent), secondary on line 1 (2-space indent)
        assert_eq!(doc.buffer.to_string(), "    aa\n    \n  bb\n  ");
    }

    #[test]
    fn insert_newline_multi_no_indent_lines() {
        let mut doc = doc_with("aa\nbb");
        doc.cursor.position = Position::new(0, 2);
        add_cursor(&mut doc, 1, 2);

        doc.insert_newline_multi();
        assert_eq!(doc.buffer.to_string(), "aa\n\nbb\n");
    }

    // ── selected_text_multi ───────────────────────────────────────

    #[test]
    fn selected_text_multi_no_selections_returns_none() {
        let mut doc = doc_with("hello");
        doc.cursor.position = Position::new(0, 3);
        add_cursor(&mut doc, 0, 1);
        assert_eq!(doc.selected_text_multi(), None);
    }

    #[test]
    fn selected_text_multi_single_cursor_delegates() {
        let mut doc = doc_with("hello");
        doc.cursor.position = Position::new(0, 5);
        doc.cursor.selection_anchor = Some(Position::new(0, 0));
        // No secondary cursors — delegates to selected_text
        assert_eq!(doc.selected_text_multi(), Some("hello".to_string()));
    }

    #[test]
    fn selected_text_multi_partial_selections() {
        let mut doc = doc_with("hello world foo");
        // Only primary has selection
        doc.cursor.position = Position::new(0, 5);
        doc.cursor.selection_anchor = Some(Position::new(0, 0));
        // Secondary has no selection
        add_cursor(&mut doc, 0, 10);

        let text = doc.selected_text_multi().unwrap();
        assert_eq!(text, "hello");
    }

    // ── merge_overlapping_cursors ─────────────────────────────────

    #[test]
    fn insert_multi_adjacent_cursors_stay_separate() {
        let mut doc = doc_with("ab");
        // Two cursors at the same position
        doc.cursor.position = Position::new(0, 1);
        add_cursor(&mut doc, 0, 1);

        doc.insert_text_multi("X");
        // Both cursors insert at position 1, resulting in "aXXb"
        assert_eq!(doc.buffer.to_string(), "aXXb");
    }

    // ── marks modified and bumps version ──────────────────────────

    #[test]
    fn insert_multi_marks_modified_and_bumps_version() {
        let mut doc = doc_with("hello");
        doc.modified = false;
        let v0 = doc.content_version;
        doc.cursor.position = Position::new(0, 5);
        add_cursor(&mut doc, 0, 0);

        doc.insert_text_multi("!");
        assert!(doc.modified);
        assert!(doc.content_version > v0);
    }

    #[test]
    fn backspace_multi_marks_modified_and_bumps_version() {
        let mut doc = doc_with("hello");
        doc.modified = false;
        let v0 = doc.content_version;
        doc.cursor.position = Position::new(0, 5);
        add_cursor(&mut doc, 0, 3);

        doc.backspace_multi();
        assert!(doc.modified);
        assert!(doc.content_version > v0);
    }

    #[test]
    fn delete_forward_multi_marks_modified_and_bumps_version() {
        let mut doc = doc_with("hello");
        doc.modified = false;
        let v0 = doc.content_version;
        doc.cursor.position = Position::new(0, 0);
        add_cursor(&mut doc, 0, 2);

        doc.delete_forward_multi();
        assert!(doc.modified);
        assert!(doc.content_version > v0);
    }
}
