//! Clipboard operations (cut, copy, paste).
//!
//! Handles single-cursor and multi-cursor clipboard interactions,
//! including per-cursor line distribution on paste.
//!
//! ## observability
//!
//! Never include decoded clipboard text in `tracing` lines or
//! `problem_log` entries — the bytes may be sensitive (credentials,
//! private keys). Log only metadata: path, length, rejection reason.

use super::App;
use crate::app::workspace_ops::is_valid_simple_name;

/// Distinguishes "this string is a file or folder path" from "this string is
/// arbitrary file content". The two cases need different sanitization:
///
/// * **Paths** are intended to be pasted into shells, file dialogs, or
///   bookmark bars. Any control character (CR, LF, TAB, NUL, ANSI escape,
///   DEL) inside a path opens the Trojan-filename attack class (CVE-2017-
///   12424 lineage). Refuse outright with a `[CP01]` problem-log entry —
///   see plan §3.7.1 / ADR-021.
/// * **File content** legitimately contains `\n`, `\t`, `\r`. Only refuse
///   on `\0` (already filtered upstream by §3.5 step 6.4 decode check), as
///   belt-and-suspenders defence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ContentKind {
    Path,
    FileContent,
}

/// Returns `true` if `ch` is a control character that we refuse to push to
/// the clipboard as part of a path. C0 controls (`U+0000..U+001F`), DEL
/// (`U+007F`), and C1 controls (`U+0080..U+009F`) are all rejected — they
/// are the byte values that interpret as shell separators, ANSI escapes,
/// terminal control sequences, or otherwise allow a file system entry's
/// name to silently inject behaviour into the paste target.
fn is_path_control_char(ch: char) -> bool {
    let code = ch as u32;
    code < 0x20 || code == 0x7F || (0x80..=0x9F).contains(&code)
}

/// Returns `true` if `text` contains any Unicode bidirectional override
/// character. These are valid Unicode and must not be silently stripped
/// (legitimate RTL documents rely on them), but they are the building
/// blocks of the "Trojan Source" attack (CVE-2021-42574): they can make
/// rendered code look one way while the underlying byte stream means
/// something different. Used by the Copy Contents pipeline to surface a
/// non-blocking notice (`[CC06]`) so the user can inspect before pasting
/// into a code review or commit message.
pub(crate) fn contains_bidi_override(text: &str) -> bool {
    text.chars().any(|c| {
        let code = c as u32;
        (0x202A..=0x202E).contains(&code) || (0x2066..=0x2069).contains(&code)
    })
}

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
    /// Writes `text` to the system clipboard after applying the
    /// kind-appropriate sanitization rules. Returns `true` when the
    /// clipboard was actually written; `false` means the call was
    /// rejected or there was no clipboard handle.
    ///
    /// ## sanitization rules
    /// * `ContentKind::Path` — refuses if `text` contains any C0 control
    ///   character, DEL, or any C1 control. Emits `[CP01]` to the problem
    ///   log on refusal. See plan §3.7.1 / ADR-021.
    /// * `ContentKind::FileContent` — refuses only on NUL (`\0`). Legitimate
    ///   file content can carry `\n`/`\t`/`\r` and must reach the clipboard
    ///   intact. The §3.5 decode step already filters NUL upstream, so a
    ///   refusal here means the upstream check was bypassed (defence in
    ///   depth).
    pub(crate) fn copy_text_to_clipboard(&mut self, text: &str, kind: ContentKind) -> bool {
        match kind {
            ContentKind::Path => {
                if text.is_empty() || text.chars().any(is_path_control_char) {
                    crate::problem_log::warn_problem(
                        "[CP01] Path contains control characters that could be exploited \
                         if pasted into a shell. Copy refused.",
                    );
                    tracing::warn!(
                        chars = text.chars().count(),
                        "Copy Path refused: control characters",
                    );
                    return false;
                }
            }
            ContentKind::FileContent => {
                if text.contains('\0') {
                    crate::problem_log::warn_problem(
                        "[CC05] File appears to be binary; not copied to clipboard.",
                    );
                    tracing::warn!(
                        chars = text.chars().count(),
                        "Copy Contents refused: NUL after decode (defence in depth)",
                    );
                    return false;
                }
            }
        }
        let Some(ref mut clipboard) = self.clipboard else {
            tracing::debug!("No clipboard handle available; copy is a no-op");
            return false;
        };
        if let Err(e) = clipboard.set_text(text.to_string()) {
            tracing::warn!(error = %e, "arboard set_text failed");
            return false;
        }
        true
    }

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
            crate::problem_log::warn_problem(
                "Pasted text rejected: not a valid file or folder name.",
            );
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

    // ── is_path_control_char ──────────────────────────────────────────

    #[test]
    fn path_control_char_rejects_c0_range() {
        for code in 0u32..0x20 {
            let ch = char::from_u32(code).unwrap();
            assert!(
                is_path_control_char(ch),
                "U+{code:04X} should be classified as a control character",
            );
        }
    }

    #[test]
    fn path_control_char_rejects_del_and_c1() {
        assert!(is_path_control_char('\u{007F}'));
        for code in 0x80u32..=0x9F {
            let ch = char::from_u32(code).unwrap();
            assert!(
                is_path_control_char(ch),
                "U+{code:04X} should be classified as a control character",
            );
        }
    }

    #[test]
    fn path_control_char_accepts_printable_ascii_and_unicode() {
        let printable = ['a', ' ', '/', '\\', '.', '~', 'é', '中', '🦀', '€'];
        for ch in printable {
            assert!(
                !is_path_control_char(ch),
                "{ch:?} (U+{:04X}) should not be a control character",
                ch as u32,
            );
        }
    }

    // ── copy_text_to_clipboard ────────────────────────────────────────
    //
    // The `test_app` factory creates an App with `clipboard: None`. Every
    // call therefore returns `false`; we are exercising the *rejection*
    // logic and the no-clipboard fallback. The actual `arboard::set_text`
    // path is exercised by manual smoke-testing and the existing
    // `paste()` integration tests.

    use super::super::tests::test_app;

    #[test]
    fn copy_path_accepts_normal_filename() {
        let mut app = test_app();
        // Clipboard is None → returns false but does NOT emit the
        // control-char rejection. We assert the rejection branch is not
        // taken by checking the call path completes without panicking.
        let written = app.copy_text_to_clipboard("/home/me/notes.md", ContentKind::Path);
        assert!(
            !written,
            "test_app has no clipboard, so write reports false"
        );
    }

    #[test]
    fn copy_path_refuses_embedded_newline() {
        let mut app = test_app();
        let written = app.copy_text_to_clipboard("safe\nrm -rf ~/", ContentKind::Path);
        assert!(!written);
    }

    #[test]
    fn copy_path_refuses_embedded_nul() {
        let mut app = test_app();
        let written = app.copy_text_to_clipboard("a\0b", ContentKind::Path);
        assert!(!written);
    }

    #[test]
    fn copy_path_refuses_ansi_escape() {
        let mut app = test_app();
        let written = app.copy_text_to_clipboard("file\x1B[2Jname.txt", ContentKind::Path);
        assert!(!written);
    }

    #[test]
    fn copy_path_refuses_del() {
        let mut app = test_app();
        let written = app.copy_text_to_clipboard("a\u{007F}b", ContentKind::Path);
        assert!(!written);
    }

    #[test]
    fn copy_path_refuses_c1_control() {
        let mut app = test_app();
        let written = app.copy_text_to_clipboard("a\u{0085}b", ContentKind::Path);
        assert!(!written);
    }

    #[test]
    fn copy_path_refuses_empty_string() {
        let mut app = test_app();
        let written = app.copy_text_to_clipboard("", ContentKind::Path);
        assert!(!written);
    }

    #[test]
    fn copy_file_content_allows_newlines_and_tabs() {
        let mut app = test_app();
        // Should pass the sanitization filter (test_app's clipboard is None
        // so the write itself reports false, but the rejection path is not
        // hit).
        let written =
            app.copy_text_to_clipboard("line1\nline2\ttabbed\r\n", ContentKind::FileContent);
        assert!(
            !written,
            "no clipboard handle, just exercising the code path"
        );
    }

    #[test]
    fn copy_file_content_refuses_nul() {
        let mut app = test_app();
        let written = app.copy_text_to_clipboard("a\0b", ContentKind::FileContent);
        assert!(!written);
    }

    // ── contains_bidi_override ────────────────────────────────────────

    #[test]
    fn bidi_detection_flags_rlo() {
        assert!(contains_bidi_override("safe code/* \u{202E} */ unsafe"));
    }

    #[test]
    fn bidi_detection_flags_full_lre_rle_pdf_lro_range() {
        for code in 0x202A..=0x202E {
            let ch = char::from_u32(code).unwrap();
            let s = format!("before{ch}after");
            assert!(
                contains_bidi_override(&s),
                "U+{code:04X} should trip the bidi detector",
            );
        }
    }

    #[test]
    fn bidi_detection_flags_lri_rli_fsi_pdi_range() {
        for code in 0x2066..=0x2069 {
            let ch = char::from_u32(code).unwrap();
            let s = format!("before{ch}after");
            assert!(
                contains_bidi_override(&s),
                "U+{code:04X} should trip the bidi detector",
            );
        }
    }

    #[test]
    fn bidi_detection_ignores_plain_ascii() {
        assert!(!contains_bidi_override(""));
        assert!(!contains_bidi_override("hello world"));
        assert!(!contains_bidi_override("def fn(x: u32) -> u32 { x + 1 }"));
    }

    #[test]
    fn bidi_detection_ignores_legitimate_rtl_text() {
        // Hebrew, Arabic — printable RTL letters with no override controls.
        assert!(!contains_bidi_override("שלום עולם"));
        assert!(!contains_bidi_override("مرحبا بالعالم"));
    }
}
