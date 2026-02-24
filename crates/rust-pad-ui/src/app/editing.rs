//! Text editing operations for the editor application.
//!
//! Bookmark navigation, case conversion, line operations (duplicate, move,
//! delete, sort), multi-cursor management, and indentation.

use rust_pad_core::buffer::TextBuffer;
use rust_pad_core::cursor::{char_to_pos, pos_to_char, Cursor, Position};
use rust_pad_core::document::Document;
use rust_pad_core::line_ops::{self, CaseConversion, SortOptions, SortOrder};

use super::context_menu::OperationScope;
use super::App;

impl App {
    /// Navigates to the next bookmark after the current cursor line.
    pub(crate) fn goto_next_bookmark(&mut self) {
        let current_line = self.tabs.active_doc().cursor.position.line;
        if let Some(line) = self.bookmarks.next(current_line) {
            let doc = self.tabs.active_doc_mut();
            doc.cursor.clear_selection();
            doc.cursor.move_to(Position::new(line, 0), &doc.buffer);
        }
    }

    /// Navigates to the previous bookmark before the current cursor line.
    pub(crate) fn goto_prev_bookmark(&mut self) {
        let current_line = self.tabs.active_doc().cursor.position.line;
        if let Some(line) = self.bookmarks.prev(current_line) {
            let doc = self.tabs.active_doc_mut();
            doc.cursor.clear_selection();
            doc.cursor.move_to(Position::new(line, 0), &doc.buffer);
        }
    }

    /// Converts the case of selected text using the given conversion mode.
    ///
    /// Handles multi-cursor (vertical) selections by converting each cursor's
    /// selection independently, processing from bottom to top to preserve offsets.
    pub(crate) fn convert_selection_case(&mut self, conversion: CaseConversion) {
        let doc = self.tabs.active_doc_mut();

        if doc.is_multi_cursor() {
            // Collect all selections as (char_start, char_end) sorted descending
            let mut ranges: Vec<(usize, usize)> = Vec::new();
            if let Ok(Some((s, e))) = doc.cursor.selection_char_range(&doc.buffer) {
                if s != e {
                    ranges.push((s, e));
                }
            }
            for sc in &doc.secondary_cursors {
                if let Ok(Some((s, e))) = sc.selection_char_range(&doc.buffer) {
                    if s != e {
                        ranges.push((s, e));
                    }
                }
            }
            if ranges.is_empty() {
                return;
            }
            // Sort descending so replacements don't shift earlier offsets
            ranges.sort_by(|a, b| b.0.cmp(&a.0));

            let snapshot = doc.snapshot_for_undo();
            let text = doc.buffer.to_string();
            let mut result = text.clone();
            for (start, end) in &ranges {
                // Convert char offsets to byte offsets in the original text
                let byte_start: usize = text.chars().take(*start).map(char::len_utf8).sum();
                let byte_end: usize = text.chars().take(*end).map(char::len_utf8).sum();
                let selected = &text[byte_start..byte_end];
                let converted = line_ops::convert_case(selected, conversion);
                // Apply to result (byte offsets are the same since we go descending
                // and each replacement has the same byte length for case conversions...
                // but actually UTF-8 case conversion can change byte lengths, so we
                // need to recalculate in the result string)
                let result_byte_start: usize =
                    result.chars().take(*start).map(char::len_utf8).sum();
                let result_byte_end: usize = result.chars().take(*end).map(char::len_utf8).sum();
                result.replace_range(result_byte_start..result_byte_end, &converted);
            }
            if result != text {
                doc.buffer = result.as_str().into();
                // Preserve all cursor selections — positions remain valid since
                // case conversion preserves character count at the same offsets.
                doc.record_undo_from_snapshot(snapshot);
            }
        } else if let Some(text) = doc.selected_text() {
            let converted = line_ops::convert_case(&text, conversion);
            if converted == text {
                return;
            }
            if let Ok(Some((start, end))) = doc.cursor.selection_char_range(&doc.buffer) {
                let snapshot = doc.snapshot_for_undo();
                let anchor = doc.cursor.selection_anchor;
                let pos = doc.cursor.position;
                let _ = doc.buffer.remove(start, end);
                let _ = doc.buffer.insert(start, &converted);
                // Restore selection, adjusting if length changed (rare for case conversion)
                let converted_chars = converted.chars().count();
                let original_chars = end - start;
                if converted_chars == original_chars {
                    doc.cursor.selection_anchor = anchor;
                    doc.cursor.position = pos;
                } else {
                    doc.cursor.selection_anchor = Some(char_to_pos(&doc.buffer, start));
                    doc.cursor.position = char_to_pos(&doc.buffer, start + converted_chars);
                }
                doc.record_undo_from_snapshot(snapshot);
            }
        }
    }

    /// Duplicates the current line, placing the copy below.
    pub(crate) fn duplicate_current_line(&mut self) {
        let doc = self.tabs.active_doc_mut();
        let snapshot = doc.snapshot_for_undo();
        let line = doc.cursor.position.line;
        if line_ops::duplicate_line(&mut doc.buffer, line).is_ok() {
            doc.cursor.position.line += 1;
            doc.record_undo_from_snapshot(snapshot);
        }
    }

    /// Moves the current line up by one position.
    pub(crate) fn move_current_line_up(&mut self) {
        let doc = self.tabs.active_doc_mut();
        let snapshot = doc.snapshot_for_undo();
        let line = doc.cursor.position.line;
        if line_ops::move_line_up(&mut doc.buffer, line).unwrap_or(false) {
            doc.cursor.position.line -= 1;
            doc.record_undo_from_snapshot(snapshot);
        }
    }

    /// Moves the current line down by one position.
    pub(crate) fn move_current_line_down(&mut self) {
        let doc = self.tabs.active_doc_mut();
        let snapshot = doc.snapshot_for_undo();
        let line = doc.cursor.position.line;
        if line_ops::move_line_down(&mut doc.buffer, line).unwrap_or(false) {
            doc.cursor.position.line += 1;
            doc.record_undo_from_snapshot(snapshot);
        }
    }

    /// Deletes the current line (Ctrl+D).
    pub(crate) fn delete_current_line(&mut self) {
        self.tabs.active_doc_mut().delete_line();
    }

    /// Selects the next occurrence of the word under cursor (Alt+Shift+.).
    ///
    /// Adds a secondary cursor at the found occurrence. Wraps around to the
    /// beginning of the document if no occurrence is found after the last cursor.
    pub(crate) fn select_next_occurrence(&mut self) {
        let doc = self.tabs.active_doc_mut();

        // Get word under primary cursor (use selection text, or select word first)
        let word = if let Some(text) = doc.selected_text() {
            text
        } else {
            doc.cursor.select_word(&doc.buffer);
            match doc.selected_text() {
                Some(t) => t,
                None => return,
            }
        };

        if word.is_empty() {
            return;
        }

        // Search from after the last cursor's position
        let last_char_idx = {
            let mut max_idx = pos_to_char(&doc.buffer, doc.cursor.position).unwrap_or(0);
            for sc in &doc.secondary_cursors {
                let idx = pos_to_char(&doc.buffer, sc.position).unwrap_or(0);
                if idx > max_idx {
                    max_idx = idx;
                }
            }
            max_idx
        };

        let text = doc.buffer.to_string();

        // Find next occurrence after last cursor
        if let Some(byte_offset) = text[last_char_idx..].find(&word) {
            // Convert byte offset to char offset
            let char_start = last_char_idx
                + text[last_char_idx..last_char_idx + byte_offset]
                    .chars()
                    .count();
            let char_end = char_start + word.chars().count();

            let mut new_cursor = Cursor::new();
            new_cursor.selection_anchor = Some(char_to_pos(&doc.buffer, char_start));
            new_cursor.position = char_to_pos(&doc.buffer, char_end);

            doc.add_secondary_cursor(new_cursor);
        } else if last_char_idx > 0 {
            // Wrap around: search from beginning
            if let Some(byte_offset) = text[..last_char_idx].find(&word) {
                let char_start = text[..byte_offset].chars().count();
                let char_end = char_start + word.chars().count();

                let mut new_cursor = Cursor::new();
                new_cursor.selection_anchor = Some(char_to_pos(&doc.buffer, char_start));
                new_cursor.position = char_to_pos(&doc.buffer, char_end);

                doc.add_secondary_cursor(new_cursor);
            }
        }
    }

    /// Adds a cursor on the line above the topmost cursor (Alt+Shift+Up).
    pub(crate) fn add_cursor_above(&mut self) {
        let doc = self.tabs.active_doc_mut();
        let min_line = std::iter::once(&doc.cursor)
            .chain(doc.secondary_cursors.iter())
            .map(|c| c.position.line)
            .min()
            .unwrap_or(0);

        if min_line == 0 {
            return;
        }

        let col = doc.cursor.position.col;
        let target_line = min_line - 1;
        let line_len = doc.buffer.line_len_chars(target_line).unwrap_or(0);

        let mut new_cursor = Cursor::new();
        new_cursor.position = Position::new(target_line, col.min(line_len));
        doc.add_secondary_cursor(new_cursor);
    }

    /// Adds a cursor on the line below the bottommost cursor (Alt+Shift+Down).
    pub(crate) fn add_cursor_below(&mut self) {
        let doc = self.tabs.active_doc_mut();
        let max_line = std::iter::once(&doc.cursor)
            .chain(doc.secondary_cursors.iter())
            .map(|c| c.position.line)
            .max()
            .unwrap_or(0);

        if max_line + 1 >= doc.buffer.len_lines() {
            return;
        }

        let col = doc.cursor.position.col;
        let target_line = max_line + 1;
        let line_len = doc.buffer.line_len_chars(target_line).unwrap_or(0);

        let mut new_cursor = Cursor::new();
        new_cursor.position = Position::new(target_line, col.min(line_len));
        doc.add_secondary_cursor(new_cursor);
    }

    /// Sorts all lines in the document in the given order.
    pub(crate) fn sort_lines(&mut self, order: SortOrder) {
        self.sort_lines_scoped(order, OperationScope::Global);
    }

    /// Removes duplicate lines from the document.
    pub(crate) fn remove_duplicate_lines(&mut self) {
        self.remove_duplicate_lines_scoped(OperationScope::Global);
    }

    /// Removes empty lines from the document.
    pub(crate) fn remove_empty_lines(&mut self) {
        self.remove_empty_lines_scoped(OperationScope::Global);
    }

    // ── Scoped operations ────────────────────────────────────────────

    /// Converts case, scoped to either the whole document or the selection.
    pub(crate) fn convert_case_scoped(
        &mut self,
        conversion: CaseConversion,
        scope: OperationScope,
    ) {
        match scope {
            OperationScope::Selection => self.convert_selection_case(conversion),
            OperationScope::Global => {
                let doc = self.tabs.active_doc_mut();
                let snapshot = doc.snapshot_for_undo();
                let text = doc.buffer.to_string();
                let converted = line_ops::convert_case(&text, conversion);
                if text != converted {
                    doc.buffer = converted.as_str().into();
                    doc.record_undo_from_snapshot(snapshot);
                }
            }
        }
    }

    /// Sorts lines, scoped to either the whole document or the selection range.
    pub(crate) fn sort_lines_scoped(&mut self, order: SortOrder, scope: OperationScope) {
        let doc = self.tabs.active_doc_mut();
        let snapshot = doc.snapshot_for_undo();
        let (start, end) = match scope {
            OperationScope::Global => (0, doc.buffer.len_lines()),
            OperationScope::Selection => selection_line_range(doc),
        };
        let opts = SortOptions {
            order,
            ..Default::default()
        };
        if line_ops::sort_lines(&mut doc.buffer, start, end, &opts).is_ok() {
            doc.record_undo_from_snapshot(snapshot);
        }
    }

    /// Removes duplicate lines, scoped to either the whole document or the selection range.
    pub(crate) fn remove_duplicate_lines_scoped(&mut self, scope: OperationScope) {
        let doc = self.tabs.active_doc_mut();
        let snapshot = doc.snapshot_for_undo();
        let (start, end) = match scope {
            OperationScope::Global => (0, doc.buffer.len_lines()),
            OperationScope::Selection => selection_line_range(doc),
        };
        if line_ops::remove_all_duplicates(&mut doc.buffer, start, end).is_ok() {
            if scope == OperationScope::Selection {
                clamp_cursors(doc);
            }
            doc.record_undo_from_snapshot(snapshot);
        }
    }

    /// Removes empty lines, scoped to either the whole document or the selection range.
    pub(crate) fn remove_empty_lines_scoped(&mut self, scope: OperationScope) {
        let doc = self.tabs.active_doc_mut();
        let snapshot = doc.snapshot_for_undo();
        let (start, end) = match scope {
            OperationScope::Global => (0, doc.buffer.len_lines()),
            OperationScope::Selection => selection_line_range(doc),
        };
        if line_ops::remove_empty_lines(&mut doc.buffer, start, end).is_ok() {
            if scope == OperationScope::Selection {
                clamp_cursors(doc);
            }
            doc.record_undo_from_snapshot(snapshot);
        }
    }

    // ── New actions ──────────────────────────────────────────────────

    /// Inverts the current selection.
    ///
    /// - No selection → select all
    /// - Entire doc selected → clear selection
    /// - Selection at start → select from sel_end to end
    /// - Selection at end → select from start to sel_start
    /// - Selection in middle → select from start to sel_start (simple inversion)
    pub(crate) fn invert_selection(&mut self) {
        let doc = self.tabs.active_doc_mut();
        let total_chars = doc.buffer.len_chars();

        let Some(sel) = doc.cursor.selection() else {
            // No selection → select all
            doc.cursor.select_all(&doc.buffer);
            return;
        };

        let sel_start = pos_to_char(&doc.buffer, sel.start()).unwrap_or(0);
        let sel_end = pos_to_char(&doc.buffer, sel.end()).unwrap_or(total_chars);

        if sel_start == 0 && sel_end >= total_chars {
            // Entire doc selected → clear
            doc.cursor.clear_selection();
            return;
        }

        if sel_start == 0 {
            // Selection at start → invert to sel_end..total_chars
            let new_start = char_to_pos(&doc.buffer, sel_end);
            let new_end = char_to_pos(&doc.buffer, total_chars);
            doc.cursor.clear_selection();
            doc.cursor.move_to(new_start, &doc.buffer);
            doc.cursor.start_selection();
            doc.cursor.move_to(new_end, &doc.buffer);
        } else if sel_end >= total_chars {
            // Selection at end → invert to 0..sel_start
            let new_start = char_to_pos(&doc.buffer, 0);
            let new_end = char_to_pos(&doc.buffer, sel_start);
            doc.cursor.clear_selection();
            doc.cursor.move_to(new_start, &doc.buffer);
            doc.cursor.start_selection();
            doc.cursor.move_to(new_end, &doc.buffer);
        } else {
            // Selection in middle → primary: 0..sel_start, secondary: sel_end..total
            let start_pos = char_to_pos(&doc.buffer, 0);
            let before_sel = char_to_pos(&doc.buffer, sel_start);
            let after_sel = char_to_pos(&doc.buffer, sel_end);
            let end_pos = char_to_pos(&doc.buffer, total_chars);

            doc.cursor.clear_selection();
            doc.cursor.move_to(start_pos, &doc.buffer);
            doc.cursor.start_selection();
            doc.cursor.move_to(before_sel, &doc.buffer);

            let mut secondary = Cursor::new();
            secondary.selection_anchor = Some(after_sel);
            secondary.position = end_pos;
            doc.add_secondary_cursor(secondary);
        }
    }

    /// Deletes the selection if one exists, otherwise deletes the next character.
    pub(crate) fn delete_selection_or_char(&mut self) {
        let doc = self.tabs.active_doc_mut();
        if doc.is_multi_cursor() {
            doc.delete_selection_multi_public();
        } else if doc.cursor.selection_anchor.is_some() {
            doc.delete_selection();
        } else {
            doc.delete_forward();
        }
    }

    /// Indents or dedents the current selection or line.
    pub(crate) fn indent_selection(&mut self, indent: bool) {
        let doc = self.tabs.active_doc_mut();
        let snapshot = doc.snapshot_for_undo();
        let sel = doc.cursor.selection();
        let (start_line, end_line) = if let Some(sel) = sel {
            (sel.start().line, sel.end().line + 1)
        } else {
            (doc.cursor.position.line, doc.cursor.position.line + 1)
        };

        let style = doc.indent_style;
        let result = if indent {
            line_ops::indent_lines(&mut doc.buffer, start_line, end_line, &style)
        } else {
            line_ops::dedent_lines(&mut doc.buffer, start_line, end_line, &style)
        };

        if result.is_ok() {
            doc.record_undo_from_snapshot(snapshot);
        }
    }
}

/// Returns the (start_line, end_line_exclusive) range covering all cursor
/// selections and positions.
///
/// When multi-cursor is active (vertical selection), the range spans from
/// the topmost cursor/selection to the bottommost. Falls back to the
/// primary cursor's line if there are no selections at all.
pub(crate) fn selection_line_range(doc: &Document) -> (usize, usize) {
    let mut min_line = doc.cursor.position.line;
    let mut max_line = doc.cursor.position.line;

    if let Some(sel) = doc.cursor.selection() {
        min_line = min_line.min(sel.start().line);
        max_line = max_line.max(sel.end().line);
    }

    for sc in &doc.secondary_cursors {
        min_line = min_line.min(sc.position.line);
        max_line = max_line.max(sc.position.line);
        if let Some(sel) = sc.selection() {
            min_line = min_line.min(sel.start().line);
            max_line = max_line.max(sel.end().line);
        }
    }

    (min_line, max_line + 1)
}

/// Clamps all cursor positions to be within buffer bounds.
///
/// Called after operations that may remove lines (e.g. remove duplicates,
/// remove empty lines) to ensure no cursor points beyond the buffer.
fn clamp_cursors(doc: &mut Document) {
    let max_line = doc.buffer.len_lines().saturating_sub(1);
    clamp_position(&mut doc.cursor.position, max_line, &doc.buffer);
    if let Some(ref mut anchor) = doc.cursor.selection_anchor {
        clamp_position(anchor, max_line, &doc.buffer);
    }
    for sc in &mut doc.secondary_cursors {
        clamp_position(&mut sc.position, max_line, &doc.buffer);
        if let Some(ref mut anchor) = sc.selection_anchor {
            clamp_position(anchor, max_line, &doc.buffer);
        }
    }
    doc.merge_overlapping_cursors();
}

/// Clamps a single position so its line and column are within buffer bounds.
fn clamp_position(pos: &mut Position, max_line: usize, buffer: &TextBuffer) {
    if pos.line > max_line {
        pos.line = max_line;
    }
    let line_len = buffer.line_len_chars(pos.line).unwrap_or(0);
    if pos.col > line_len {
        pos.col = line_len;
    }
}
