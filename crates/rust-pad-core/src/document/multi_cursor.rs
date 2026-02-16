//! Multi-cursor editing operations for documents.
//!
//! Provides text insertion, deletion, and selection operations that work
//! across multiple cursor positions simultaneously.

use crate::cursor::{char_to_pos, pos_to_char};

use super::Document;

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

    /// Deletes selections at the given cursor positions (must be sorted descending).
    fn delete_selections_at_cursors(&mut self, indices: &[(usize, usize, bool)]) {
        for &(_, idx, is_primary) in indices {
            let cursor = if is_primary {
                &self.cursor
            } else {
                &self.secondary_cursors[idx]
            };
            if let Ok(Some((start, end))) = cursor.selection_char_range(&self.buffer) {
                if start != end {
                    let _ = self.buffer.remove(start, end);
                }
            }
        }
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

    /// Collects cursor positions using the minimum of anchor and position.
    ///
    /// Used after selection deletion to find where inserts should happen.
    fn collect_min_selection_positions(&self) -> Vec<(usize, usize, bool)> {
        let mut indices = Vec::with_capacity(1 + self.secondary_cursors.len());
        let primary_pos = self
            .cursor
            .selection_anchor
            .unwrap_or(self.cursor.position)
            .min(self.cursor.position);
        if let Ok(idx) = pos_to_char(&self.buffer, primary_pos) {
            indices.push((idx, 0, true));
        }
        for (i, sc) in self.secondary_cursors.iter().enumerate() {
            let pos = sc.selection_anchor.unwrap_or(sc.position).min(sc.position);
            if let Ok(idx) = pos_to_char(&self.buffer, pos) {
                indices.push((idx, i, false));
            }
        }
        indices
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

        let mut cursor_indices = self.collect_cursor_indices();
        cursor_indices.sort_by(|a, b| b.0.cmp(&a.0));

        let insert_len = text.chars().count();

        // Delete selections first (in reverse order), then insert
        self.delete_selections_at_cursors(&cursor_indices);

        // Recalculate positions after deletions and insert in reverse order
        let mut cursor_indices2 = self.collect_min_selection_positions();
        cursor_indices2.sort_by(|a, b| b.0.cmp(&a.0));

        for &(char_idx, _, _) in &cursor_indices2 {
            let _ = self.buffer.insert(char_idx, text);
        }

        // Sort ascending, track cumulative offset to update each cursor
        cursor_indices2.sort_by_key(|&(idx, _, _)| idx);

        let mut offset = 0usize;
        for &(original_idx, src_idx, is_primary) in &cursor_indices2 {
            let new_char_idx = original_idx + offset + insert_len;
            let new_pos = char_to_pos(&self.buffer, new_char_idx);
            self.set_cursor_position(src_idx, is_primary, new_pos, true);
            offset += insert_len;
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

        // Build (char_idx, src_idx, is_primary) sorted ascending by document position
        let mut cursor_info = self.collect_cursor_indices();
        cursor_info.sort_by_key(|&(idx, _, _)| idx);

        // Map each sorted position to its text in document order
        let text_assignments: Vec<(usize, &str, usize, bool)> = cursor_info
            .iter()
            .enumerate()
            .map(|(text_idx, &(char_idx, src_idx, is_primary))| {
                (char_idx, texts[text_idx], src_idx, is_primary)
            })
            .collect();

        // Delete selections first (reverse document order)
        let mut desc = self.collect_cursor_indices();
        desc.sort_by(|a, b| b.0.cmp(&a.0));
        self.delete_selections_at_cursors(&desc);

        // Recalculate positions after deletions, preserving text assignment order
        let positions_in_order: Vec<(usize, bool)> = text_assignments
            .iter()
            .map(|&(_, _, src_idx, is_primary)| (src_idx, is_primary))
            .collect();

        let mut assignments2: Vec<(usize, &str, usize, bool)> = Vec::new();
        for (text_idx, &(src_idx, is_primary)) in positions_in_order.iter().enumerate() {
            let pos = if is_primary {
                self.cursor
                    .selection_anchor
                    .unwrap_or(self.cursor.position)
                    .min(self.cursor.position)
            } else {
                let sc = &self.secondary_cursors[src_idx];
                sc.selection_anchor.unwrap_or(sc.position).min(sc.position)
            };
            if let Ok(idx) = pos_to_char(&self.buffer, pos) {
                assignments2.push((idx, texts[text_idx], src_idx, is_primary));
            }
        }

        // Insert in reverse document order so earlier positions stay valid
        assignments2.sort_by(|a, b| b.0.cmp(&a.0));
        for &(char_idx, text, _, _) in &assignments2 {
            let _ = self.buffer.insert(char_idx, text);
        }

        // Update cursor positions ascending with cumulative offset
        assignments2.sort_by_key(|&(idx, _, _, _)| idx);
        let mut offset = 0usize;
        for &(original_idx, text, src_idx, is_primary) in &assignments2 {
            let insert_len = text.chars().count();
            let new_char_idx = original_idx + offset + insert_len;
            let new_pos = char_to_pos(&self.buffer, new_char_idx);
            self.set_cursor_position(src_idx, is_primary, new_pos, true);
            offset += insert_len;
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

    /// Inserts a newline at all cursor positions.
    pub fn insert_newline_multi(&mut self) {
        if self.secondary_cursors.is_empty() {
            self.insert_newline();
            return;
        }
        self.insert_text_multi("\n");
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
