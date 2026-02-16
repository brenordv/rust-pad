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
            match key {
                // === Always-active shortcuts (file, zoom, tabs, search open) ===

                // File operations
                egui::Key::N if ctrl => self.new_tab(),
                egui::Key::O if ctrl => self.open_file_dialog(),
                egui::Key::S if ctrl && shift => self.save_as_dialog(),
                egui::Key::S if ctrl => self.save_active(),
                egui::Key::W if ctrl => {
                    let active = self.tabs.active;
                    self.request_close_tab(active);
                }

                // Search (open dialog)
                egui::Key::F if ctrl => self.find_replace.open(),
                egui::Key::H if ctrl => self.find_replace.open(),
                egui::Key::G if ctrl => self.go_to_line.open(),

                // Zoom
                egui::Key::Plus if ctrl => {
                    self.zoom_level = (self.zoom_level + 0.1).min(self.max_zoom_level);
                }
                egui::Key::Minus if ctrl => {
                    self.zoom_level = (self.zoom_level - 0.1).max(0.5);
                }
                egui::Key::Num0 if ctrl => self.zoom_level = 1.0,

                // Tab switching
                egui::Key::Tab if ctrl && shift => {
                    let count = self.tabs.tab_count();
                    let prev = (self.tabs.active + count - 1) % count;
                    self.tabs.switch_to(prev);
                }
                egui::Key::Tab if ctrl => {
                    let next = (self.tabs.active + 1) % self.tabs.tab_count();
                    self.tabs.switch_to(next);
                }

                // Escape closes dialogs and clears multi-cursor
                egui::Key::Escape => {
                    self.find_replace.close();
                    self.go_to_line.visible = false;
                    self.tabs.active_doc_mut().clear_secondary_cursors();
                }

                // === Editor-only shortcuts (suppressed when a dialog is open) ===

                // Edit operations
                egui::Key::Z if ctrl && !dialog_open => self.tabs.active_doc_mut().undo(),
                egui::Key::Y if ctrl && !dialog_open => self.tabs.active_doc_mut().redo(),
                egui::Key::X if ctrl && !dialog_open => self.cut(),
                egui::Key::C if ctrl && !dialog_open => self.copy(),
                egui::Key::V if ctrl && !dialog_open => self.paste(),
                egui::Key::A if ctrl && !dialog_open => {
                    let doc = self.tabs.active_doc_mut();
                    doc.cursor.select_all(&doc.buffer);
                    doc.clear_secondary_cursors();
                }

                // Bookmarks
                egui::Key::F2 if ctrl && !dialog_open => {
                    let line = self.tabs.active_doc().cursor.position.line;
                    self.bookmarks.toggle(line);
                }
                egui::Key::F2 if shift && !dialog_open => self.goto_prev_bookmark(),
                egui::Key::F2 if !dialog_open => self.goto_next_bookmark(),

                // Delete current line
                egui::Key::D if ctrl && !dialog_open => self.delete_current_line(),

                // Multi-cursor: select next occurrence of word
                egui::Key::Period if alt && shift && !dialog_open => {
                    self.select_next_occurrence();
                }

                // Multi-cursor: add cursor above/below
                egui::Key::ArrowUp if alt && shift && !dialog_open => self.add_cursor_above(),
                egui::Key::ArrowDown if alt && shift && !dialog_open => self.add_cursor_below(),

                // Line movement (only when shift is NOT held, to avoid conflict)
                egui::Key::ArrowUp if alt && !dialog_open => self.move_current_line_up(),
                egui::Key::ArrowDown if alt && !dialog_open => self.move_current_line_down(),

                _ => {}
            }
        }
    }
}
