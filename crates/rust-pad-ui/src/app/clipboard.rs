//! Clipboard operations (cut, copy, paste).
//!
//! Handles single-cursor and multi-cursor clipboard interactions,
//! including per-cursor line distribution on paste.

use super::App;
use crate::app::workspace_ops::is_valid_simple_name;

/// Sanitizes clipboard text destined for a single-segment filename field.
///
/// Strips CR / LF / NUL characters and trims surrounding whitespace.
/// Returns `Some(sanitized)` when the result is a valid simple name,
/// otherwise `None` so the caller can reject the paste with a
/// problem-log entry.
fn sanitize_clipboard_for_filename(text: &str) -> Option<String> {
    let cleaned: String = text
        .chars()
        .filter(|c| *c != '\r' && *c != '\n' && *c != '\0')
        .collect();
    let trimmed = cleaned.trim().to_string();
    if is_valid_simple_name(&trimmed) {
        Some(trimmed)
    } else {
        None
    }
}

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

    /// Synthesizes an `egui::Event::Paste` for a Ctrl+V on a focused
    /// inline workspace field on macOS, where egui's native TextEdit
    /// paste handler only fires on `Cmd+V`. Sanitizes the clipboard
    /// content per V5 (security review); rejected pastes are surfaced
    /// to the Problems dialog so the user understands why nothing
    /// happened.
    pub(crate) fn inject_inline_paste(&mut self, ctx: &egui::Context) {
        let Some(ref mut clipboard) = self.clipboard else {
            tracing::debug!("Inline paste: no clipboard handle available");
            return;
        };
        let text = match clipboard.get_text() {
            Ok(t) => t,
            Err(e) => {
                tracing::debug!("Inline paste: clipboard read failed: {e}");
                return;
            }
        };
        let Some(sanitized) = sanitize_clipboard_for_filename(&text) else {
            let msg = "Pasted text rejected: not a valid file or folder name.";
            tracing::warn!("{msg}");
            crate::problem_log::log_problem(msg);
            return;
        };
        ctx.input_mut(|i| i.events.push(egui::Event::Paste(sanitized)));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_strips_cr_lf_nul() {
        assert_eq!(
            sanitize_clipboard_for_filename("foo\r\nbar"),
            Some("foobar".to_string())
        );
        assert_eq!(
            sanitize_clipboard_for_filename("a\0b"),
            Some("ab".to_string())
        );
    }

    #[test]
    fn sanitize_trims_whitespace() {
        assert_eq!(
            sanitize_clipboard_for_filename("  hello.txt  "),
            Some("hello.txt".to_string())
        );
    }

    #[test]
    fn sanitize_rejects_path_traversal() {
        assert_eq!(sanitize_clipboard_for_filename("../escape"), None);
        assert_eq!(sanitize_clipboard_for_filename("/etc/passwd"), None);
        assert_eq!(sanitize_clipboard_for_filename("a/b"), None);
        assert_eq!(sanitize_clipboard_for_filename("C:\\Win"), None);
    }

    #[test]
    fn sanitize_rejects_empty_after_strip() {
        assert_eq!(sanitize_clipboard_for_filename(""), None);
        assert_eq!(sanitize_clipboard_for_filename("\r\n"), None);
        assert_eq!(sanitize_clipboard_for_filename("   "), None);
    }

    #[test]
    fn sanitize_accepts_normal_filenames() {
        assert_eq!(
            sanitize_clipboard_for_filename("notes.md"),
            Some("notes.md".to_string())
        );
        assert_eq!(
            sanitize_clipboard_for_filename(".env"),
            Some(".env".to_string())
        );
    }
}
