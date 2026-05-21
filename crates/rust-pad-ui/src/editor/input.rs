//! Keyboard input handling for the editor widget.
//!
//! Processes key events (text insertion, cursor movement, editing commands)
//! and maps them to document operations.

use egui::Ui;

use rust_pad_core::buffer::TextBuffer;
use rust_pad_core::cursor::Cursor;

use super::widget::EditorWidget;
use super::wrap_map::WrapMap;

impl<'a> EditorWidget<'a> {
    /// Handles all keyboard input for the editor widget.
    ///
    /// `wrap_map` is the frame's word-wrap map (`Some` when word-wrap is on),
    /// used to make vertical arrow navigation visual-line-aware.
    pub(crate) fn handle_keyboard_input(&mut self, ui: &mut Ui, wrap_map: Option<&WrapMap>) {
        let events: Vec<egui::Event> = ui.input(|i| i.events.clone());

        // Reset cursor blink on any keyboard activity
        if !events.is_empty() {
            self.doc.cursor_activity_time = ui.input(|i| i.time);
        }

        for event in &events {
            match event {
                egui::Event::Text(text) => {
                    // Suppress text insertion when ctrl or alt is held — those
                    // key combos are handled as shortcuts, not text input.
                    // Without the alt check, Alt+Shift+. produces a ">" text
                    // event that gets inserted into the document.
                    if !ui.input(|i| i.modifiers.ctrl || i.modifiers.command || i.modifiers.alt) {
                        if self.doc.is_multi_cursor() {
                            self.doc.insert_text_multi(text);
                        } else {
                            self.doc.insert_text(text);
                        }
                    }
                }
                egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } => {
                    self.handle_key(*key, *modifiers, wrap_map);
                }
                _ => {}
            }
        }
    }

    /// Handles a single key press.
    fn handle_key(
        &mut self,
        key: egui::Key,
        modifiers: egui::Modifiers,
        wrap_map: Option<&WrapMap>,
    ) {
        let shift = modifiers.shift;
        let ctrl = modifiers.ctrl || modifiers.command;
        let alt = modifiers.alt;

        // Alt+Arrow combos are handled by global shortcuts (line movement, multi-cursor).
        // Skip widget-level processing to avoid double-moving cursors or unwanted selection.
        if alt
            && matches!(
                key,
                egui::Key::ArrowUp
                    | egui::Key::ArrowDown
                    | egui::Key::ArrowLeft
                    | egui::Key::ArrowRight
            )
        {
            return;
        }

        let is_movement = matches!(
            key,
            egui::Key::ArrowLeft
                | egui::Key::ArrowRight
                | egui::Key::ArrowUp
                | egui::Key::ArrowDown
                | egui::Key::Home
                | egui::Key::End
                | egui::Key::PageUp
                | egui::Key::PageDown
        );

        // Selection mode: if shift is held, start/extend selection on all cursors
        if shift && is_movement {
            self.doc.cursor.start_selection();
            for sc in &mut self.doc.secondary_cursors {
                sc.start_selection();
            }
        } else if !shift && is_movement {
            self.doc.cursor.clear_selection();
            for sc in &mut self.doc.secondary_cursors {
                sc.clear_selection();
            }
        }

        // Try each handler group; return early once one matches.
        if self.handle_navigation_key(key, ctrl, wrap_map) {
            return;
        }
        if self.handle_editing_key(key, ctrl, shift) {
            return;
        }
        self.handle_selection_key(key, ctrl);
        // Note: Ctrl+Z/Y/X/C/V are handled by the App level
    }

    /// Handles navigation keys (arrows, Home, End, PageUp, PageDown).
    /// Returns `true` if the key was consumed.
    fn handle_navigation_key(
        &mut self,
        key: egui::Key,
        ctrl: bool,
        wrap_map: Option<&WrapMap>,
    ) -> bool {
        match key {
            egui::Key::ArrowLeft if ctrl => {
                self.doc.cursor.move_word_left(&self.doc.buffer);
                for sc in &mut self.doc.secondary_cursors {
                    sc.move_word_left(&self.doc.buffer);
                }
            }
            egui::Key::ArrowRight if ctrl => {
                self.doc.cursor.move_word_right(&self.doc.buffer);
                for sc in &mut self.doc.secondary_cursors {
                    sc.move_word_right(&self.doc.buffer);
                }
            }
            egui::Key::ArrowLeft => {
                self.doc.cursor.move_left(&self.doc.buffer);
                for sc in &mut self.doc.secondary_cursors {
                    sc.move_left(&self.doc.buffer);
                }
            }
            egui::Key::ArrowRight => {
                self.doc.cursor.move_right(&self.doc.buffer);
                for sc in &mut self.doc.secondary_cursors {
                    sc.move_right(&self.doc.buffer);
                }
            }
            egui::Key::ArrowUp => self.move_cursors_vertical(wrap_map, true),
            egui::Key::ArrowDown => self.move_cursors_vertical(wrap_map, false),
            egui::Key::Home if ctrl => {
                self.doc.cursor.move_to_start();
                self.doc.clear_secondary_cursors();
            }
            egui::Key::End if ctrl => {
                self.doc.cursor.move_to_end(&self.doc.buffer);
                self.doc.clear_secondary_cursors();
            }
            egui::Key::Home => {
                self.doc.cursor.move_to_line_start();
                for sc in &mut self.doc.secondary_cursors {
                    sc.move_to_line_start();
                }
            }
            egui::Key::End => {
                self.doc.cursor.move_to_line_end(&self.doc.buffer);
                for sc in &mut self.doc.secondary_cursors {
                    sc.move_to_line_end(&self.doc.buffer);
                }
            }
            egui::Key::PageUp => {
                self.doc.cursor.move_page_up(30, &self.doc.buffer);
                self.doc.clear_secondary_cursors();
            }
            egui::Key::PageDown => {
                self.doc.cursor.move_page_down(30, &self.doc.buffer);
                self.doc.clear_secondary_cursors();
            }
            _ => return false,
        }
        true
    }

    /// Moves the primary and every secondary cursor one line vertically.
    /// In wrap mode (`Some(wm)`) movement is by visual (wrapped) line; otherwise
    /// it falls back to the logical `Cursor::move_up`/`move_down` (unchanged
    /// wrap-off behaviour).
    fn move_cursors_vertical(&mut self, wrap_map: Option<&WrapMap>, up: bool) {
        match wrap_map {
            Some(wm) => {
                move_cursor_visual(&mut self.doc.cursor, &self.doc.buffer, wm, up);
                for sc in &mut self.doc.secondary_cursors {
                    move_cursor_visual(sc, &self.doc.buffer, wm, up);
                }
            }
            None => {
                if up {
                    self.doc.cursor.move_up(&self.doc.buffer);
                } else {
                    self.doc.cursor.move_down(&self.doc.buffer);
                }
                for sc in &mut self.doc.secondary_cursors {
                    if up {
                        sc.move_up(&self.doc.buffer);
                    } else {
                        sc.move_down(&self.doc.buffer);
                    }
                }
            }
        }
    }

    /// Handles editing keys (Enter, Backspace, Delete, Tab).
    /// Returns `true` if the key was consumed.
    fn handle_editing_key(&mut self, key: egui::Key, ctrl: bool, shift: bool) -> bool {
        match key {
            egui::Key::Enter => {
                if self.doc.is_multi_cursor() {
                    self.doc.insert_newline_multi();
                } else {
                    self.doc.insert_newline();
                }
            }
            egui::Key::Backspace if ctrl => {
                // Delete word left
                self.doc.cursor.start_selection();
                self.doc.cursor.move_word_left(&self.doc.buffer);
                self.doc.delete_selection();
            }
            egui::Key::Backspace => {
                if self.doc.is_multi_cursor() {
                    self.doc.backspace_multi();
                } else {
                    self.doc.backspace();
                }
            }
            egui::Key::Delete if ctrl => {
                // Delete word right
                self.doc.cursor.start_selection();
                self.doc.cursor.move_word_right(&self.doc.buffer);
                self.doc.delete_selection();
            }
            egui::Key::Delete => {
                if self.doc.is_multi_cursor() {
                    self.doc.delete_forward_multi();
                } else {
                    self.doc.delete_forward();
                }
            }
            egui::Key::Tab if shift => {
                // Selection-aware dedent; otherwise fall back to dedenting
                // the cursor's current line (Notepad++-style Shift+Tab).
                if !self.doc.indent_or_dedent_selection(false) {
                    self.dedent_line();
                }
            }
            egui::Key::Tab => {
                // Selection-aware indent; otherwise insert indent text at
                // every cursor (the historical single/multi-cursor behavior).
                if !self.doc.indent_or_dedent_selection(true) {
                    let indent = self.doc.indent_style.indent_text();
                    if self.doc.is_multi_cursor() {
                        self.doc.insert_text_multi(&indent);
                    } else {
                        self.doc.insert_text(&indent);
                    }
                }
            }
            _ => return false,
        }
        true
    }

    /// Handles selection keys (Ctrl+A to select all).
    fn handle_selection_key(&mut self, key: egui::Key, ctrl: bool) {
        if key == egui::Key::A && ctrl {
            self.doc.cursor.select_all(&self.doc.buffer);
            self.doc.clear_secondary_cursors();
        }
    }

    /// Removes one level of indentation from the current line.
    fn dedent_line(&mut self) {
        let line_idx = self.doc.cursor.position.line;
        let style = self.doc.indent_style;
        let removed =
            rust_pad_core::line_ops::leading_indent_removable(&self.doc.buffer, line_idx, &style);
        if removed > 0 {
            let line_start = self.doc.buffer.line_to_char(line_idx).unwrap_or(0);
            if self
                .doc
                .buffer
                .remove(line_start, line_start + removed)
                .is_ok()
            {
                self.doc.cursor.position.col = self.doc.cursor.position.col.saturating_sub(removed);
                self.doc.modified = true;
            }
        }
    }
}

/// Moves `cursor` by one *visual* (wrapped) line using the frame's `WrapMap`.
/// `up == true` moves toward the document start. Mirrors `Cursor::move_up` /
/// `Cursor::move_down`: it early-returns at the first/last visual line, keeps a
/// sticky column, and clamps the column to the target segment's length.
///
/// The sticky column reuses `Cursor::desired_col`. The *desired visual column*
/// is `desired % chars_per_visual_line`; `desired` itself is preserved across
/// consecutive presses (exactly as the logical moves do), so the original
/// visual column survives passing through shorter segments.
fn move_cursor_visual(cursor: &mut Cursor, buffer: &TextBuffer, wm: &WrapMap, up: bool) {
    let cur_visual = wm.position_to_visual_line(cursor.position.line, cursor.position.col);
    let target_visual = if up {
        if cur_visual == 0 {
            return; // already on the first visual line — matches move_up's edge guard
        }
        cur_visual - 1
    } else {
        if cur_visual + 1 >= wm.total_visual_lines {
            return; // already on the last visual line — matches move_down's edge guard
        }
        cur_visual + 1
    };

    let cpvl = wm.chars_per_visual_line;
    let desired = cursor.desired_col.unwrap_or(cursor.position.col);
    let desired_visual_col = desired % cpvl;

    let (logical_line, wrap_row) = wm.visual_to_logical(target_visual);
    // line_len_chars already excludes the trailing newline, matching the
    // content length WrapMap uses to compute segments.
    let content_len = buffer.line_len_chars(logical_line).unwrap_or(0);
    let seg_start = wrap_row * cpvl;
    // Only the last segment of a logical line can be shorter than cpvl; clamp
    // the desired visual column into it. desired_visual_col < cpvl guarantees
    // we never spill into the next visual row.
    let seg_end = (seg_start + cpvl).min(content_len);

    cursor.position.line = logical_line;
    cursor.position.col = (seg_start + desired_visual_col).min(seg_end);
    cursor.desired_col = Some(desired);
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_pad_core::cursor::Position;
    use rust_pad_core::document::Document;

    /// Builds a `Document` containing `text` and a `WrapMap` with the given
    /// chars-per-visual-line. Deterministic — no viewport-width dependency.
    fn doc_and_wrap(text: &str, cpvl: usize) -> (Document, WrapMap) {
        let mut doc = Document::new();
        doc.insert_text(text);
        let wm = WrapMap::build(&doc, cpvl);
        (doc, wm)
    }

    #[test]
    fn move_down_visual_steps_within_wrapped_line() {
        // 25-char single logical line → 3 visual rows of width 10.
        let (mut doc, wm) = doc_and_wrap("0123456789012345678901234", 10);
        doc.cursor.position = Position::new(0, 3);

        move_cursor_visual(&mut doc.cursor, &doc.buffer, &wm, false);
        assert_eq!(doc.cursor.position, Position::new(0, 13));
        move_cursor_visual(&mut doc.cursor, &doc.buffer, &wm, false);
        assert_eq!(doc.cursor.position, Position::new(0, 23));
        // Already on the last visual row → no-op.
        move_cursor_visual(&mut doc.cursor, &doc.buffer, &wm, false);
        assert_eq!(doc.cursor.position, Position::new(0, 23));
    }

    #[test]
    fn move_up_visual_steps_within_wrapped_line() {
        let (mut doc, wm) = doc_and_wrap("0123456789012345678901234", 10);
        doc.cursor.position = Position::new(0, 23);

        move_cursor_visual(&mut doc.cursor, &doc.buffer, &wm, true);
        assert_eq!(doc.cursor.position, Position::new(0, 13));
        move_cursor_visual(&mut doc.cursor, &doc.buffer, &wm, true);
        assert_eq!(doc.cursor.position, Position::new(0, 3));
    }

    #[test]
    fn move_down_visual_crosses_into_next_logical_line() {
        // Line 0: 20 chars → 2 visual rows; line 1: "abc".
        let (mut doc, wm) = doc_and_wrap("01234567890123456789\nabc", 10);
        // On the last visual row of line 0, visual col 5.
        doc.cursor.position = Position::new(0, 15);

        move_cursor_visual(&mut doc.cursor, &doc.buffer, &wm, false);
        // Sticky visual col 5 clamps to the 3-char next line.
        assert_eq!(doc.cursor.position, Position::new(1, 3));
    }

    #[test]
    fn move_up_visual_crosses_into_previous_logical_line() {
        // Line 0: "abc"; line 1: 20 chars → 2 visual rows.
        let (mut doc, wm) = doc_and_wrap("abc\n01234567890123456789", 10);
        // First visual row of line 1, visual col 2.
        doc.cursor.position = Position::new(1, 2);

        move_cursor_visual(&mut doc.cursor, &doc.buffer, &wm, true);
        assert_eq!(doc.cursor.position, Position::new(0, 2));
    }

    #[test]
    fn visual_sticky_column_preserved_through_short_segment() {
        // Three single-row logical lines: wide, short (4), wide.
        let (mut doc, wm) = doc_and_wrap("abcdefghij\nwxyz\nABCDEFGHIJ", 10);
        doc.cursor.position = Position::new(0, 7);

        // Down into the 4-char line clamps the column to 4.
        move_cursor_visual(&mut doc.cursor, &doc.buffer, &wm, false);
        assert_eq!(doc.cursor.position, Position::new(1, 4));
        // desired_col is preserved at the original 7.
        assert_eq!(doc.cursor.desired_col, Some(7));
        // Down into the next wide line restores visual col 7.
        move_cursor_visual(&mut doc.cursor, &doc.buffer, &wm, false);
        assert_eq!(doc.cursor.position, Position::new(2, 7));
    }

    #[test]
    fn move_up_visual_at_first_visual_line_is_noop() {
        let (mut doc, wm) = doc_and_wrap("0123456789012345678901234", 10);
        doc.cursor.position = Position::new(0, 3);
        move_cursor_visual(&mut doc.cursor, &doc.buffer, &wm, true);
        assert_eq!(doc.cursor.position, Position::new(0, 3));
    }

    #[test]
    fn move_down_visual_at_last_visual_line_is_noop() {
        let (mut doc, wm) = doc_and_wrap("abc\ndef", 10);
        doc.cursor.position = Position::new(1, 2);
        move_cursor_visual(&mut doc.cursor, &doc.buffer, &wm, false);
        assert_eq!(doc.cursor.position, Position::new(1, 2));
    }

    #[test]
    fn move_visual_with_no_actual_wrapping_matches_logical() {
        // cpvl large enough that nothing wraps → identical to logical movement.
        let (mut doc, wm) = doc_and_wrap("abc\ndef\nghi", 80);
        doc.cursor.position = Position::new(1, 2);

        let mut logical = doc.cursor.clone();
        logical.move_down(&doc.buffer);
        move_cursor_visual(&mut doc.cursor, &doc.buffer, &wm, false);
        assert_eq!(doc.cursor.position, logical.position);

        let mut logical_up = doc.cursor.clone();
        logical_up.move_up(&doc.buffer);
        move_cursor_visual(&mut doc.cursor, &doc.buffer, &wm, true);
        assert_eq!(doc.cursor.position, logical_up.position);
    }

    #[test]
    fn move_down_visual_clamps_desired_visual_col_into_last_short_segment() {
        // 24-char line → rows 0(0-9), 1(10-19), 2(20-23, 4 chars).
        let (mut doc, wm) = doc_and_wrap("012345678901234567890123", 10);
        doc.cursor.position = Position::new(0, 8);

        // Step down to row 1, then into the short last row (row 2).
        move_cursor_visual(&mut doc.cursor, &doc.buffer, &wm, false);
        assert_eq!(doc.cursor.position, Position::new(0, 18));
        // Visual col 8 would be char 28, clamped into the 4-char segment end 24.
        move_cursor_visual(&mut doc.cursor, &doc.buffer, &wm, false);
        assert_eq!(doc.cursor.position, Position::new(0, 24));
        let content_len = doc.buffer.line_len_chars(0).unwrap_or(0);
        assert!(doc.cursor.position.col <= content_len);
    }

    #[test]
    fn move_cursor_visual_applies_to_secondary_cursors() {
        // The wrap-on path in `move_cursors_vertical` runs `move_cursor_visual`
        // over the primary and each secondary cursor; verify it is cursor-
        // agnostic so multi-cursor vertical movement stays in parity (D5).
        let (mut doc, wm) = doc_and_wrap("0123456789012345678901234", 10);
        doc.cursor.position = Position::new(0, 2);
        let mut secondary = Cursor::new();
        secondary.position = Position::new(0, 5);

        move_cursor_visual(&mut doc.cursor, &doc.buffer, &wm, false);
        move_cursor_visual(&mut secondary, &doc.buffer, &wm, false);

        assert_eq!(doc.cursor.position, Position::new(0, 12));
        assert_eq!(secondary.position, Position::new(0, 15));
    }
}
