//! Keyboard input handling for the editor widget.
//!
//! Processes key events (text insertion, cursor movement, editing commands)
//! and maps them to document operations.

use egui::Ui;

use super::widget::EditorWidget;

impl<'a> EditorWidget<'a> {
    /// Handles all keyboard input for the editor widget.
    pub(crate) fn handle_keyboard_input(&mut self, ui: &mut Ui) {
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
                    self.handle_key(*key, *modifiers);
                }
                _ => {}
            }
        }
    }

    /// Handles a single key press.
    fn handle_key(&mut self, key: egui::Key, modifiers: egui::Modifiers) {
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
        if self.handle_navigation_key(key, ctrl) {
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
    fn handle_navigation_key(&mut self, key: egui::Key, ctrl: bool) -> bool {
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
            egui::Key::ArrowUp => {
                self.doc.cursor.move_up(&self.doc.buffer);
                for sc in &mut self.doc.secondary_cursors {
                    sc.move_up(&self.doc.buffer);
                }
            }
            egui::Key::ArrowDown => {
                self.doc.cursor.move_down(&self.doc.buffer);
                for sc in &mut self.doc.secondary_cursors {
                    sc.move_down(&self.doc.buffer);
                }
            }
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
                // Dedent
                self.dedent_line();
            }
            egui::Key::Tab => {
                let indent = self.doc.indent_style.indent_text();
                if self.doc.is_multi_cursor() {
                    self.doc.insert_text_multi(&indent);
                } else {
                    self.doc.insert_text(&indent);
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
        if let Ok(line) = self.doc.buffer.line(line_idx) {
            let line_str = line.to_string();
            let removed = match self.doc.indent_style {
                rust_pad_core::indent::IndentStyle::Tabs => {
                    if line_str.starts_with('\t') {
                        1
                    } else {
                        // Fall back: remove up to 4 leading spaces for mixed content
                        line_str.chars().take_while(|c| *c == ' ').count().min(4)
                    }
                }
                rust_pad_core::indent::IndentStyle::Spaces(n) => {
                    line_str.chars().take_while(|c| *c == ' ').count().min(n)
                }
            };

            if removed > 0 {
                let line_start = self.doc.buffer.line_to_char(line_idx).unwrap_or(0);
                if self
                    .doc
                    .buffer
                    .remove(line_start, line_start + removed)
                    .is_ok()
                {
                    self.doc.cursor.position.col =
                        self.doc.cursor.position.col.saturating_sub(removed);
                    self.doc.modified = true;
                }
            }
        }
    }
}
