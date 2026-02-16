//! Text editing operations for the editor application.
//!
//! Bookmark navigation, case conversion, line operations (duplicate, move,
//! delete, sort), multi-cursor management, and indentation.

use rust_pad_core::cursor::{char_to_pos, pos_to_char, Cursor, Position};
use rust_pad_core::line_ops::{self, CaseConversion, SortOptions, SortOrder};

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
    pub(crate) fn convert_selection_case(&mut self, conversion: CaseConversion) {
        let doc = self.tabs.active_doc_mut();
        if let Some(text) = doc.selected_text() {
            let converted = line_ops::convert_case(&text, conversion);
            doc.insert_text(&converted);
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
        let doc = self.tabs.active_doc_mut();
        let snapshot = doc.snapshot_for_undo();
        let total = doc.buffer.len_lines();
        let opts = SortOptions {
            order,
            ..Default::default()
        };
        if line_ops::sort_lines(&mut doc.buffer, 0, total, &opts).is_ok() {
            doc.record_undo_from_snapshot(snapshot);
        }
    }

    /// Removes duplicate lines from the document.
    pub(crate) fn remove_duplicate_lines(&mut self) {
        let doc = self.tabs.active_doc_mut();
        let snapshot = doc.snapshot_for_undo();
        let total = doc.buffer.len_lines();
        if line_ops::remove_all_duplicates(&mut doc.buffer, 0, total).is_ok() {
            doc.record_undo_from_snapshot(snapshot);
        }
    }

    /// Removes empty lines from the document.
    pub(crate) fn remove_empty_lines(&mut self) {
        let doc = self.tabs.active_doc_mut();
        let snapshot = doc.snapshot_for_undo();
        let total = doc.buffer.len_lines();
        if line_ops::remove_empty_lines(&mut doc.buffer, 0, total).is_ok() {
            doc.record_undo_from_snapshot(snapshot);
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
