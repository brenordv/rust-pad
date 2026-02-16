//! Editor theme configuration.
//!
//! Defines the visual appearance of the editor widget, including colors
//! for text, cursor, selection, gutter, scrollbars, and special characters.

use egui::{Color32, FontId};
use rust_pad_config::{EditorColors, HexColor};

/// Converts a `HexColor` to egui `Color32`.
fn hex_to_color32(c: HexColor) -> Color32 {
    Color32::from_rgba_premultiplied(c.r, c.g, c.b, c.a)
}

/// Configuration for the editor widget appearance.
#[derive(Debug, Clone)]
pub struct EditorTheme {
    pub font_size: f32,
    pub font_id: FontId,
    pub bg_color: Color32,
    pub text_color: Color32,
    pub cursor_color: Color32,
    pub selection_color: Color32,
    pub line_number_color: Color32,
    pub line_number_bg: Color32,
    pub gutter_width: f32,
    pub current_line_highlight: Color32,
    pub modified_line_color: Color32,
    pub saved_line_color: Color32,
    pub show_change_tracking: bool,
    pub gutter_separator_color: Color32,
    pub scrollbar_track_color: Color32,
    pub scrollbar_thumb_idle: Color32,
    pub scrollbar_thumb_hover: Color32,
    pub scrollbar_thumb_active: Color32,
    pub occurrence_highlight_color: Color32,
    pub special_char_color: Color32,
}

impl Default for EditorTheme {
    fn default() -> Self {
        Self::dark()
    }
}

impl EditorTheme {
    /// Dark theme preset (uses `EditorColors::default()`).
    pub fn dark() -> Self {
        Self::from_config(&EditorColors::default(), 14.0)
    }

    /// Builds an `EditorTheme` from config colors and font size.
    pub fn from_config(editor: &EditorColors, font_size: f32) -> Self {
        Self {
            font_size,
            font_id: FontId::monospace(font_size),
            bg_color: hex_to_color32(editor.bg_color),
            text_color: hex_to_color32(editor.text_color),
            cursor_color: hex_to_color32(editor.cursor_color),
            selection_color: hex_to_color32(editor.selection_color),
            line_number_color: hex_to_color32(editor.line_number_color),
            line_number_bg: hex_to_color32(editor.line_number_bg),
            gutter_width: 50.0,
            current_line_highlight: hex_to_color32(editor.current_line_highlight),
            modified_line_color: hex_to_color32(editor.modified_line_color),
            saved_line_color: hex_to_color32(editor.saved_line_color),
            show_change_tracking: false,
            gutter_separator_color: hex_to_color32(editor.gutter_separator_color),
            scrollbar_track_color: hex_to_color32(editor.scrollbar_track_color),
            scrollbar_thumb_idle: hex_to_color32(editor.scrollbar_thumb_idle),
            scrollbar_thumb_hover: hex_to_color32(editor.scrollbar_thumb_hover),
            scrollbar_thumb_active: hex_to_color32(editor.scrollbar_thumb_active),
            occurrence_highlight_color: hex_to_color32(editor.occurrence_highlight_color),
            special_char_color: hex_to_color32(editor.special_char_color),
        }
    }

    /// Light theme preset (uses config-crate `builtin_light()` colors).
    pub fn light() -> Self {
        Self::from_config(&rust_pad_config::theme::builtin_light().editor, 14.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Default / dark ─────────────────────────────────────────────

    #[test]
    fn default_is_dark() {
        let def = EditorTheme::default();
        let dark = EditorTheme::dark();
        assert_eq!(def.bg_color, dark.bg_color);
        assert_eq!(def.text_color, dark.text_color);
        assert_eq!(def.font_size, dark.font_size);
    }

    #[test]
    fn dark_has_expected_font_size() {
        let theme = EditorTheme::dark();
        assert!((theme.font_size - 14.0).abs() < f32::EPSILON);
    }

    #[test]
    fn dark_bg_is_dark() {
        let theme = EditorTheme::dark();
        // Dark theme bg should be dark (low RGB values)
        assert_eq!(theme.bg_color, Color32::from_rgb(30, 30, 30));
    }

    #[test]
    fn dark_text_is_light() {
        let theme = EditorTheme::dark();
        assert_eq!(theme.text_color, Color32::from_rgb(212, 212, 212));
    }

    // ── Light theme ────────────────────────────────────────────────

    #[test]
    fn light_bg_is_white() {
        let theme = EditorTheme::light();
        assert_eq!(theme.bg_color, Color32::from_rgb(255, 255, 255));
    }

    #[test]
    fn light_text_is_dark() {
        let theme = EditorTheme::light();
        assert_eq!(theme.text_color, Color32::from_rgb(30, 30, 30));
    }

    #[test]
    fn light_has_expected_font_size() {
        let theme = EditorTheme::light();
        assert!((theme.font_size - 14.0).abs() < f32::EPSILON);
    }

    #[test]
    fn dark_and_light_differ() {
        let dark = EditorTheme::dark();
        let light = EditorTheme::light();
        assert_ne!(dark.bg_color, light.bg_color);
        assert_ne!(dark.text_color, light.text_color);
        assert_ne!(dark.cursor_color, light.cursor_color);
    }

    // ── from_config ────────────────────────────────────────────────

    #[test]
    fn from_config_uses_provided_font_size() {
        let config = EditorColors::default();
        let theme = EditorTheme::from_config(&config, 20.0);
        assert!((theme.font_size - 20.0).abs() < f32::EPSILON);
        assert_eq!(theme.font_id, FontId::monospace(20.0));
    }

    #[test]
    fn from_config_uses_config_colors() {
        let mut config = EditorColors::default();
        config.bg_color = HexColor::rgb(100, 100, 100);
        let theme = EditorTheme::from_config(&config, 14.0);
        assert_eq!(theme.bg_color, Color32::from_rgb(100, 100, 100));
    }

    #[test]
    fn from_config_default_matches_dark() {
        let config = EditorColors::default();
        let from_cfg = EditorTheme::from_config(&config, 14.0);
        let dark = EditorTheme::dark();
        // The default EditorColors should produce colors matching the dark theme
        assert_eq!(from_cfg.bg_color, dark.bg_color);
        assert_eq!(from_cfg.text_color, dark.text_color);
        assert_eq!(from_cfg.cursor_color, dark.cursor_color);
    }

    #[test]
    fn from_config_show_change_tracking_defaults_false() {
        let config = EditorColors::default();
        let theme = EditorTheme::from_config(&config, 14.0);
        assert!(!theme.show_change_tracking);
    }

    #[test]
    fn from_config_gutter_width_is_50() {
        let config = EditorColors::default();
        let theme = EditorTheme::from_config(&config, 14.0);
        assert!((theme.gutter_width - 50.0).abs() < f32::EPSILON);
    }

    // ── Clone ──────────────────────────────────────────────────────

    #[test]
    fn theme_clone_produces_equal_copy() {
        let theme = EditorTheme::dark();
        let cloned = theme.clone();
        assert_eq!(theme.bg_color, cloned.bg_color);
        assert_eq!(theme.text_color, cloned.text_color);
        assert_eq!(theme.font_size, cloned.font_size);
    }
}
