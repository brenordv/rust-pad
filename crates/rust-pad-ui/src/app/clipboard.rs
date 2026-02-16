//! Clipboard operations (cut, copy, paste).
//!
//! Handles single-cursor and multi-cursor clipboard interactions,
//! including per-cursor line distribution on paste.

use super::App;

impl App {
    /// Cuts selected text: copies to clipboard then deletes the selection.
    pub(crate) fn cut(&mut self) {
        self.copy();
        let doc = self.tabs.active_doc_mut();
        if doc.is_multi_cursor() {
            doc.delete_selection_multi_public();
        } else {
            doc.delete_selection();
        }
    }

    /// Copies selected text to the system clipboard.
    pub(crate) fn copy(&mut self) {
        let doc = self.tabs.active_doc();
        let text = if doc.is_multi_cursor() {
            doc.selected_text_multi()
        } else {
            doc.selected_text()
        };
        if let Some(text) = text {
            if let Some(ref mut clipboard) = self.clipboard {
                let _ = clipboard.set_text(text);
            }
        }
    }

    /// Pastes clipboard content at cursor positions.
    ///
    /// In multi-cursor mode, distributes one line per cursor when the clipboard
    /// line count matches the cursor count; otherwise pastes the full text at
    /// each cursor.
    pub(crate) fn paste(&mut self) {
        if let Some(ref mut clipboard) = self.clipboard {
            if let Ok(text) = clipboard.get_text() {
                let normalized = rust_pad_core::encoding::normalize_line_endings(&text);
                let doc = self.tabs.active_doc_mut();

                if doc.is_multi_cursor() {
                    let lines: Vec<&str> = normalized.split('\n').collect();
                    let cursor_count = 1 + doc.secondary_cursors.len();

                    if lines.len() == cursor_count {
                        // Distribute one line per cursor (in document order)
                        doc.insert_text_per_cursor(&lines);
                    } else {
                        // Line count doesn't match cursor count: paste full text at each cursor
                        doc.insert_text_multi(&normalized);
                    }
                } else {
                    doc.insert_text(&normalized);
                }
            }
        }
    }
}
