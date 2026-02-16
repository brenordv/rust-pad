//! Multi-cursor editing operations for documents.
//!
//! Provides text insertion, deletion, and selection operations that work
//! across multiple cursor positions simultaneously.

use crate::cursor::{char_to_pos, pos_to_char};

use super::Document;

impl Document {
    /// Inserts text at all cursor positions (primary + secondary).
    pub fn insert_text_multi(&mut self, text: &str) {
        if self.secondary_cursors.is_empty() {
            self.insert_text(text);
            return;
        }

        // Collect all cursor char indices with their source
        let mut cursor_indices: Vec<(usize, usize, bool)> = Vec::new(); // (char_idx, index, is_primary)

        if let Ok(idx) = self.cursor.to_char_index(&self.buffer) {
            cursor_indices.push((idx, 0, true));
        }
        for (i, sc) in self.secondary_cursors.iter().enumerate() {
            if let Ok(idx) = sc.to_char_index(&self.buffer) {
                cursor_indices.push((idx, i, false));
            }
        }

        // Sort descending by char index so edits don't shift earlier positions
        cursor_indices.sort_by(|a, b| b.0.cmp(&a.0));

        let insert_len = text.chars().count();

        // Delete selections first (in reverse order), then insert
        for &(_, idx, is_primary) in &cursor_indices {
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

        // Recalculate positions after deletions and insert in reverse order
        let mut cursor_indices2: Vec<(usize, usize, bool)> = Vec::new();
        if let Ok(idx) = pos_to_char(
            &self.buffer,
            self.cursor
                .selection_anchor
                .unwrap_or(self.cursor.position)
                .min(self.cursor.position),
        ) {
            cursor_indices2.push((idx, 0, true));
        }
        for (i, sc) in self.secondary_cursors.iter().enumerate() {
            let pos = sc.selection_anchor.unwrap_or(sc.position).min(sc.position);
            if let Ok(idx) = pos_to_char(&self.buffer, pos) {
                cursor_indices2.push((idx, i, false));
            }
        }
        cursor_indices2.sort_by(|a, b| b.0.cmp(&a.0));

        for &(char_idx, _, _) in &cursor_indices2 {
            let _ = self.buffer.insert(char_idx, text);
        }

        // Sort ascending, track cumulative offset to update each cursor
        let mut ascending: Vec<(usize, usize, bool)> = cursor_indices2.clone();
        ascending.sort_by(|a, b| a.0.cmp(&b.0));

        let mut offset = 0usize;
        for &(original_idx, src_idx, is_primary) in &ascending {
            let new_char_idx = original_idx + offset + insert_len;
            let new_pos = char_to_pos(&self.buffer, new_char_idx);
            if is_primary {
                self.cursor.position = new_pos;
                self.cursor.clear_selection();
                self.cursor.desired_col = None;
            } else {
                self.secondary_cursors[src_idx].position = new_pos;
                self.secondary_cursors[src_idx].clear_selection();
                self.secondary_cursors[src_idx].desired_col = None;
            }
            offset += insert_len;
        }

        self.merge_overlapping_cursors();
        self.sync_line_changes();
        self.modified = true;
        self.scroll_to_cursor = true;
        self.bump_version();
    }

    /// Inserts a different string at each cursor position (primary + secondary).
    ///
    /// `texts` must have exactly `1 + secondary_cursors.len()` elements:
    /// index 0 for the primary cursor, then one per secondary in order.
    pub fn insert_text_per_cursor(&mut self, texts: &[&str]) {
        let cursor_count = 1 + self.secondary_cursors.len();
        if texts.len() != cursor_count {
            // Fallback: insert the first text at all cursors
            if let Some(first) = texts.first() {
                self.insert_text_multi(first);
            }
            return;
        }

        if self.secondary_cursors.is_empty() {
            self.insert_text(texts[0]);
            return;
        }

        // Build (char_idx, cursor_index_in_texts, is_primary) sorted by document position
        let mut cursor_info: Vec<(usize, usize, bool)> = Vec::new();
        if let Ok(idx) = self.cursor.to_char_index(&self.buffer) {
            cursor_info.push((idx, 0, true));
        }
        for (i, sc) in self.secondary_cursors.iter().enumerate() {
            if let Ok(idx) = sc.to_char_index(&self.buffer) {
                cursor_info.push((idx, i, false));
            }
        }

        // Sort ascending by char index to assign texts in document order
        cursor_info.sort_by_key(|&(idx, _, _)| idx);

        // Map each sorted position to its text: the first cursor in document
        // order gets texts[0], the second gets texts[1], etc.
        let text_assignments: Vec<(usize, &str, usize, bool)> = cursor_info
            .iter()
            .enumerate()
            .map(|(text_idx, &(char_idx, src_idx, is_primary))| {
                (char_idx, texts[text_idx], src_idx, is_primary)
            })
            .collect();

        // Delete selections first (reverse order)
        let mut desc = text_assignments.clone();
        desc.sort_by(|a, b| b.0.cmp(&a.0));

        for &(_, _, src_idx, is_primary) in &desc {
            let cursor = if is_primary {
                &self.cursor
            } else {
                &self.secondary_cursors[src_idx]
            };
            if let Ok(Some((start, end))) = cursor.selection_char_range(&self.buffer) {
                if start != end {
                    let _ = self.buffer.remove(start, end);
                }
            }
        }

        // Recalculate positions after deletions, preserving text assignment order
        let mut assignments2: Vec<(usize, &str, usize, bool)> = Vec::new();
        // Rebuild in the same document-order as text_assignments
        let positions_in_order: Vec<(usize, bool)> = text_assignments
            .iter()
            .map(|&(_, _, src_idx, is_primary)| (src_idx, is_primary))
            .collect();

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
            if is_primary {
                self.cursor.position = new_pos;
                self.cursor.clear_selection();
                self.cursor.desired_col = None;
            } else {
                self.secondary_cursors[src_idx].position = new_pos;
                self.secondary_cursors[src_idx].clear_selection();
                self.secondary_cursors[src_idx].desired_col = None;
            }
            offset += insert_len;
        }

        self.merge_overlapping_cursors();
        self.sync_line_changes();
        self.modified = true;
        self.scroll_to_cursor = true;
        self.bump_version();
    }

    /// Performs backspace at all cursor positions.
    pub fn backspace_multi(&mut self) {
        if self.secondary_cursors.is_empty() {
            self.backspace();
            return;
        }

        // If any cursor has a selection, delete selections instead
        let has_selection = self.cursor.selection_anchor.is_some()
            || self
                .secondary_cursors
                .iter()
                .any(|c| c.selection_anchor.is_some());

        if has_selection {
            self.delete_selection_multi();
            return;
        }

        // Collect char indices, sort descending
        let mut indices: Vec<(usize, usize, bool)> = Vec::new();
        if let Ok(idx) = self.cursor.to_char_index(&self.buffer) {
            indices.push((idx, 0, true));
        }
        for (i, sc) in self.secondary_cursors.iter().enumerate() {
            if let Ok(idx) = sc.to_char_index(&self.buffer) {
                indices.push((idx, i, false));
            }
        }
        indices.sort_by(|a, b| b.0.cmp(&a.0));

        // Delete one char before each cursor (in reverse order)
        for &(char_idx, _, _) in &indices {
            if char_idx > 0 {
                let _ = self.buffer.remove(char_idx - 1, char_idx);
            }
        }

        // Update positions ascending with cumulative offset
        let mut ascending = indices.clone();
        ascending.sort_by(|a, b| a.0.cmp(&b.0));

        let mut deleted_count = 0usize;
        for &(original_idx, src_idx, is_primary) in &ascending {
            if original_idx > 0 {
                let new_char_idx = original_idx - 1 - deleted_count;
                let new_pos = char_to_pos(&self.buffer, new_char_idx);
                if is_primary {
                    self.cursor.position = new_pos;
                    self.cursor.desired_col = None;
                } else {
                    self.secondary_cursors[src_idx].position = new_pos;
                    self.secondary_cursors[src_idx].desired_col = None;
                }
                deleted_count += 1;
            } else {
                // Cursor at position 0, can't backspace but still need to adjust for prior deletions
                let new_pos = char_to_pos(&self.buffer, original_idx.saturating_sub(deleted_count));
                if is_primary {
                    self.cursor.position = new_pos;
                } else {
                    self.secondary_cursors[src_idx].position = new_pos;
                }
            }
        }

        self.merge_overlapping_cursors();
        self.sync_line_changes();
        self.modified = true;
        self.bump_version();
    }

    /// Deletes selections at all cursors (public alias).
    pub fn delete_selection_multi_public(&mut self) {
        self.delete_selection_multi();
    }

    /// Deletes selections at all cursors.
    fn delete_selection_multi(&mut self) {
        // Collect selection ranges, sort descending by start
        let mut ranges: Vec<(usize, usize, usize, bool)> = Vec::new(); // (start, end, idx, is_primary)
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

        // Update positions
        let mut ascending = ranges.clone();
        ascending.sort_by(|a, b| a.0.cmp(&b.0));

        let mut offset = 0usize;
        for &(start, end, src_idx, is_primary) in &ascending {
            let new_pos = char_to_pos(&self.buffer, start.saturating_sub(offset));
            if is_primary {
                self.cursor.position = new_pos;
                self.cursor.clear_selection();
                self.cursor.desired_col = None;
            } else {
                self.secondary_cursors[src_idx].position = new_pos;
                self.secondary_cursors[src_idx].clear_selection();
                self.secondary_cursors[src_idx].desired_col = None;
            }
            offset += end - start;
        }

        // Also clear selection on cursors that had no selection
        self.cursor.clear_selection();
        for sc in &mut self.secondary_cursors {
            sc.clear_selection();
        }

        self.merge_overlapping_cursors();
        self.sync_line_changes();
        self.modified = true;
        self.bump_version();
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
        let mut indices: Vec<(usize, usize, bool)> = Vec::new();
        if let Ok(idx) = self.cursor.to_char_index(&self.buffer) {
            indices.push((idx, 0, true));
        }
        for (i, sc) in self.secondary_cursors.iter().enumerate() {
            if let Ok(idx) = sc.to_char_index(&self.buffer) {
                indices.push((idx, i, false));
            }
        }
        indices.sort_by(|a, b| b.0.cmp(&a.0));

        for &(char_idx, _, _) in &indices {
            if char_idx < total {
                let _ = self.buffer.remove(char_idx, char_idx + 1);
            }
        }

        // Update positions ascending
        let mut ascending = indices.clone();
        ascending.sort_by(|a, b| a.0.cmp(&b.0));

        let mut deleted_count = 0usize;
        for &(original_idx, src_idx, is_primary) in &ascending {
            let new_char_idx = original_idx.saturating_sub(deleted_count);
            let new_pos = char_to_pos(&self.buffer, new_char_idx);
            if is_primary {
                self.cursor.position = new_pos;
                self.cursor.desired_col = None;
            } else {
                self.secondary_cursors[src_idx].position = new_pos;
                self.secondary_cursors[src_idx].desired_col = None;
            }
            if original_idx < total {
                deleted_count += 1;
            }
        }

        self.merge_overlapping_cursors();
        self.sync_line_changes();
        self.modified = true;
        self.bump_version();
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
