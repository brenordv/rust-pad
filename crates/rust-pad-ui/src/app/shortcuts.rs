//! Global keyboard shortcut handling.
//!
//! Processes key events and maps them to application actions such as
//! file operations, clipboard, zoom, tabs, search, and editing.

use eframe::egui;

use super::{App, DialogState};

impl App {
    /// Returns true if any dialog is currently open and capturing input.
    pub(crate) fn is_dialog_open(&self) -> bool {
        self.find_replace.visible
            || self.go_to_line.visible
            || self.settings_open
            || self.about_open
            || matches!(self.dialog_state, DialogState::ConfirmClose(_))
    }

    /// Handles global keyboard shortcuts.
    pub(crate) fn handle_global_shortcuts(&mut self, ctx: &egui::Context) {
        // Collect key events and semantic clipboard events.
        // When a widget has focus, egui converts Ctrl+C/V/X into Event::Copy/Paste/Cut
        // instead of raw Key events, so we must detect both forms.
        let (ctrl, shift, alt, keys, has_copy, has_cut, has_paste) = ctx.input(|i| {
            let ctrl = i.modifiers.ctrl || i.modifiers.command;
            let shift = i.modifiers.shift;
            let alt = i.modifiers.alt;
            let mut has_copy = false;
            let mut has_cut = false;
            let mut has_paste = false;
            let keys: Vec<egui::Key> = i
                .events
                .iter()
                .filter_map(|e| match e {
                    egui::Event::Key {
                        key, pressed: true, ..
                    } => Some(*key),
                    egui::Event::Copy => {
                        has_copy = true;
                        None
                    }
                    egui::Event::Cut => {
                        has_cut = true;
                        None
                    }
                    egui::Event::Paste(ref _text) => {
                        has_paste = true;
                        None
                    }
                    _ => None,
                })
                .collect();
            (ctrl, shift, alt, keys, has_copy, has_cut, has_paste)
        });

        // Handle semantic clipboard events (from focused widget)
        if !self.is_dialog_open() {
            if has_copy {
                self.copy();
            }
            if has_cut {
                self.cut();
            }
            if has_paste {
                self.paste();
            }
        }

        let dialog_open = self.is_dialog_open();

        for key in &keys {
            // Always-active shortcuts first, then editor-only shortcuts.
            if self.handle_file_shortcut(*key, ctrl, shift) {
                continue;
            }
            if self.handle_search_shortcut(*key, ctrl) {
                continue;
            }
            if self.handle_zoom_shortcut(*key, ctrl) {
                continue;
            }
            if self.handle_tab_shortcut(*key, ctrl, shift) {
                continue;
            }
            if self.handle_escape_shortcut(*key) {
                continue;
            }

            // Editor-only shortcuts are suppressed when a dialog is open.
            if dialog_open {
                continue;
            }
            if self.handle_edit_shortcut(*key, ctrl) {
                continue;
            }
            if self.handle_bookmark_shortcut(*key, ctrl, shift) {
                continue;
            }
            self.handle_multicursor_and_line_shortcut(*key, alt, shift);
        }
    }

    /// File operation shortcuts (Ctrl+N, Ctrl+O, Ctrl+S, Ctrl+Shift+S, Ctrl+W).
    /// Returns `true` if the key was consumed.
    fn handle_file_shortcut(&mut self, key: egui::Key, ctrl: bool, shift: bool) -> bool {
        if !ctrl {
            return false;
        }
        match key {
            egui::Key::N => self.new_tab(),
            egui::Key::O => self.open_file_dialog(),
            egui::Key::S if shift => self.save_as_dialog(),
            egui::Key::S => self.save_active(),
            egui::Key::W => {
                let active = self.tabs.active;
                self.request_close_tab(active);
            }
            _ => return false,
        }
        true
    }

    /// Search dialog shortcuts (Ctrl+F, Ctrl+H, Ctrl+G).
    /// Returns `true` if the key was consumed.
    fn handle_search_shortcut(&mut self, key: egui::Key, ctrl: bool) -> bool {
        if !ctrl {
            return false;
        }
        match key {
            egui::Key::F | egui::Key::H => self.find_replace.open(),
            egui::Key::G => self.go_to_line.open(),
            _ => return false,
        }
        true
    }

    /// Zoom shortcuts (Ctrl+Plus, Ctrl+Minus, Ctrl+0).
    /// Returns `true` if the key was consumed.
    fn handle_zoom_shortcut(&mut self, key: egui::Key, ctrl: bool) -> bool {
        if !ctrl {
            return false;
        }
        match key {
            egui::Key::Plus => {
                self.zoom_level = (self.zoom_level + 0.1).min(self.max_zoom_level);
            }
            egui::Key::Minus => {
                self.zoom_level = (self.zoom_level - 0.1).max(0.5);
            }
            egui::Key::Num0 => self.zoom_level = 1.0,
            _ => return false,
        }
        true
    }

    /// Tab switching shortcuts (Ctrl+Tab, Ctrl+Shift+Tab).
    /// Returns `true` if the key was consumed.
    fn handle_tab_shortcut(&mut self, key: egui::Key, ctrl: bool, shift: bool) -> bool {
        if key != egui::Key::Tab || !ctrl {
            return false;
        }
        if shift {
            let count = self.tabs.tab_count();
            let prev = (self.tabs.active + count - 1) % count;
            self.tabs.switch_to(prev);
        } else {
            let next = (self.tabs.active + 1) % self.tabs.tab_count();
            self.tabs.switch_to(next);
        }
        true
    }

    /// Escape key: closes dialogs and clears multi-cursor.
    /// Returns `true` if the key was consumed.
    fn handle_escape_shortcut(&mut self, key: egui::Key) -> bool {
        if key != egui::Key::Escape {
            return false;
        }
        self.find_replace.close();
        self.go_to_line.visible = false;
        self.tabs.active_doc_mut().clear_secondary_cursors();
        true
    }

    /// Edit shortcuts (Ctrl+Z, Ctrl+Y, Ctrl+X, Ctrl+C, Ctrl+V, Ctrl+A, Ctrl+D).
    /// Returns `true` if the key was consumed.
    fn handle_edit_shortcut(&mut self, key: egui::Key, ctrl: bool) -> bool {
        if !ctrl {
            return false;
        }
        match key {
            egui::Key::Z => self.tabs.active_doc_mut().undo(),
            egui::Key::Y => self.tabs.active_doc_mut().redo(),
            egui::Key::X => self.cut(),
            egui::Key::C => self.copy(),
            egui::Key::V => self.paste(),
            egui::Key::A => {
                let doc = self.tabs.active_doc_mut();
                doc.cursor.select_all(&doc.buffer);
                doc.clear_secondary_cursors();
            }
            egui::Key::D => self.delete_current_line(),
            _ => return false,
        }
        true
    }

    /// Bookmark shortcuts (F2, Ctrl+F2, Shift+F2).
    /// Returns `true` if the key was consumed.
    fn handle_bookmark_shortcut(&mut self, key: egui::Key, ctrl: bool, shift: bool) -> bool {
        if key != egui::Key::F2 {
            return false;
        }
        if ctrl {
            let line = self.tabs.active_doc().cursor.position.line;
            self.bookmarks.toggle(line);
        } else if shift {
            self.goto_prev_bookmark();
        } else {
            self.goto_next_bookmark();
        }
        true
    }

    /// Multi-cursor and line movement shortcuts (Alt+Shift+Period, Alt+Shift+Arrow, Alt+Arrow).
    fn handle_multicursor_and_line_shortcut(&mut self, key: egui::Key, alt: bool, shift: bool) {
        if !alt {
            return;
        }
        if shift {
            match key {
                egui::Key::Period => self.select_next_occurrence(),
                egui::Key::ArrowUp => self.add_cursor_above(),
                egui::Key::ArrowDown => self.add_cursor_below(),
                _ => {}
            }
        } else {
            // Line movement (only when shift is NOT held, to avoid conflict)
            match key {
                egui::Key::ArrowUp => self.move_current_line_up(),
                egui::Key::ArrowDown => self.move_current_line_down(),
                _ => {}
            }
        }
    }
}
