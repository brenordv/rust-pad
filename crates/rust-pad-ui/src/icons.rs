//! Centralized vocabulary of UI icons.
//!
//! Every icon used by the application is referenced through a named constant
//! in this module. The constants are `&'static str` slices containing the
//! Phosphor codepoint(s) for the icon, sourced from the `egui-phosphor` crate
//! (Regular weight). Going through this module instead of inlining codepoints
//! at call sites means we can swap icon families or weights in one place if
//! we ever need to.
//!
//! See `plan §4.3` (`.task-manager/phase-18-plan.md`) for the mapping table.
//!
//! ## Why constants, not an enum
//!
//! Icons are pasted into `RichText::new(...)` and `format!(...)` strings; an
//! enum would force every call site through a converter. `&'static str` is
//! the natural shape for that usage pattern.

use egui_phosphor::regular as ph;

// ── Tree entries ──────────────────────────────────────────────────────

/// Folder icon used for both collapsed and expanded directory entries.
pub const FOLDER: &str = ph::FOLDER;
/// Generic file icon (default fallback for unknown extensions).
pub const FILE: &str = ph::FILE;
/// Code file (`.rs`, etc.).
pub const FILE_CODE: &str = ph::FILE_CODE;
/// Text file (`.md`, `.txt`, `.log`).
pub const FILE_TEXT: &str = ph::FILE_TEXT;
/// Image file (`.png`, `.jpg`, etc.).
pub const FILE_IMAGE: &str = ph::FILE_IMAGE;
/// Configuration file (`.toml`, `.yaml`, `.json`, `.xml`).
pub const GEAR: &str = ph::GEAR;
/// Locked file (`.lock`).
pub const LOCK: &str = ph::LOCK;
/// Warning marker for unavailable workspace folders.
pub const WARNING_CIRCLE: &str = ph::WARNING_CIRCLE;

// ── Inline entry-creation icons ──────────────────────────────────────

/// Inline "new file" prompt icon.
pub const FILE_PLUS: &str = ph::FILE_PLUS;
/// Inline "new folder" prompt icon.
pub const FOLDER_PLUS: &str = ph::FOLDER_PLUS;

// ── Sidebar toolbar ──────────────────────────────────────────────────

/// Close button (workspace close).
pub const X: &str = ph::X;
/// Add button (add folder).
pub const PLUS: &str = ph::PLUS;
/// Hidden files visible state.
pub const EYE: &str = ph::EYE;
/// Hidden files hidden state.
pub const EYE_SLASH: &str = ph::EYE_SLASH;
/// Collapse-all caret.
pub const CARET_DOUBLE_UP: &str = ph::CARET_DOUBLE_UP;
/// Expand-all caret.
pub const CARET_DOUBLE_DOWN: &str = ph::CARET_DOUBLE_DOWN;
/// Active-workspace check marker.
pub const CHECK: &str = ph::CHECK;

#[cfg(test)]
mod tests {
    use super::*;

    /// Every icon constant currently referenced by the application. Kept as
    /// the single source of truth for the two smoke tests below; if a
    /// future call site adds a new icon, append it here so the coverage
    /// stays honest.
    const ALL: &[(&str, &str)] = &[
        (FOLDER, "FOLDER"),
        (FILE, "FILE"),
        (FILE_CODE, "FILE_CODE"),
        (FILE_TEXT, "FILE_TEXT"),
        (FILE_IMAGE, "FILE_IMAGE"),
        (GEAR, "GEAR"),
        (LOCK, "LOCK"),
        (WARNING_CIRCLE, "WARNING_CIRCLE"),
        (FILE_PLUS, "FILE_PLUS"),
        (FOLDER_PLUS, "FOLDER_PLUS"),
        (X, "X"),
        (PLUS, "PLUS"),
        (EYE, "EYE"),
        (EYE_SLASH, "EYE_SLASH"),
        (CARET_DOUBLE_UP, "CARET_DOUBLE_UP"),
        (CARET_DOUBLE_DOWN, "CARET_DOUBLE_DOWN"),
        (CHECK, "CHECK"),
    ];

    /// Smoke test: every icon constant resolves to a non-empty string. If a
    /// Phosphor rename ever removes one of the upstream constants this
    /// fails to compile — the assertion is a belt to the suspenders.
    #[test]
    fn all_constants_are_non_empty() {
        for (value, name) in ALL {
            assert!(
                !value.is_empty(),
                "icon constant {name} resolved to an empty string",
            );
        }
    }

    /// Catches accidental mass-aliasing: two different upstream codepoints
    /// must not collapse to the same constant.
    #[test]
    fn distinct_constants_have_distinct_codepoints() {
        for (i, (a_val, a_name)) in ALL.iter().enumerate() {
            for (b_val, b_name) in ALL.iter().skip(i + 1) {
                assert_ne!(
                    a_val, b_val,
                    "icon {a_name} and {b_name} share the same codepoint",
                );
            }
        }
    }
}
