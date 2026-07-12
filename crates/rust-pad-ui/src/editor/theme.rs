//! Editor theme configuration.
//!
//! Defines the visual appearance of the editor widget, including colors
//! for text, cursor, selection, gutter, scrollbars, and special characters.

use egui::{Color32, FontId};
use rust_pad_config::EditorColors;

use crate::app::resolved_theme::{color32, ChromeTheme, EolMarkerStyle, Metrics, MetricsStyle};

/// Marker color for bookmarked lines in the gutter; not part of the theme
/// token set, so it stays a constant shared by every theme.
const BOOKMARK_MARKER_COLOR: Color32 = Color32::from_rgb(66, 133, 244);

/// Line height multiplier used before line height became a theme metric;
/// preset constructors and legacy themes keep it.
pub const LEGACY_LINE_HEIGHT_FACTOR: f32 = 1.4;

/// Secondary-caret color used before it became themeable; legacy themes
/// keep it.
const LEGACY_SECONDARY_CURSOR_COLOR: Color32 = Color32::from_rgb(200, 200, 200);

/// Gutter number size relative to the editor font in redesigned themes
/// (12 px numbers against the default 14 px editor font, scaling with the
/// user's font size and zoom).
const CHROME_GUTTER_FONT_FACTOR: f32 = 12.0 / 14.0;
/// Gutter number right padding, pre-redesign.
const LEGACY_GUTTER_NUMBER_PADDING: f32 = 8.0;
/// Gutter number right padding in redesigned themes.
const CHROME_GUTTER_NUMBER_PADDING: f32 = 13.0;

/// Configuration for the editor widget appearance.
#[derive(Debug, Clone)]
pub struct EditorTheme {
    pub font_size: f32,
    pub font_id: FontId,
    /// Line height as a multiple of the font size.
    pub line_height_factor: f32,
    pub bg_color: Color32,
    pub text_color: Color32,
    pub cursor_color: Color32,
    pub selection_color: Color32,
    pub line_number_color: Color32,
    pub line_number_bg: Color32,
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
    pub matching_bracket_color: Color32,
    pub special_char_color: Color32,
    pub bookmark_marker_color: Color32,
    /// Color of the line number on rows carrying a cursor.
    pub active_line_number_color: Color32,
    /// Gutter number font size as a fraction of the editor font size.
    pub gutter_font_factor: f32,
    /// Space between the line number and the gutter's right edge.
    pub gutter_number_padding: f32,
    /// Caret color for secondary (multi-cursor) carets.
    pub secondary_cursor_color: Color32,
    /// How line endings are marked when special characters are shown.
    pub eol_marker: EolMarkerStyle,
    /// Fill of the compact EOL chip (Chip marker style).
    pub crlf_chip_bg: Color32,
    /// Text color of the compact EOL chip.
    pub crlf_chip_text: Color32,
}

impl Default for EditorTheme {
    fn default() -> Self {
        Self::dark()
    }
}

impl EditorTheme {
    /// Dark theme preset (uses `EditorColors::default()`).
    pub fn dark() -> Self {
        Self::from_config(&EditorColors::default(), 14.0, LEGACY_LINE_HEIGHT_FACTOR)
    }

    /// Builds an `EditorTheme` from config colors, font size, and the
    /// theme-metric line height factor.
    pub fn from_config(editor: &EditorColors, font_size: f32, line_height_factor: f32) -> Self {
        Self {
            font_size,
            font_id: FontId::monospace(font_size),
            line_height_factor,
            bg_color: color32(editor.bg_color),
            text_color: color32(editor.text_color),
            cursor_color: color32(editor.cursor_color),
            selection_color: color32(editor.selection_color),
            line_number_color: color32(editor.line_number_color),
            line_number_bg: color32(editor.line_number_bg),
            current_line_highlight: color32(editor.current_line_highlight),
            modified_line_color: color32(editor.modified_line_color),
            saved_line_color: color32(editor.saved_line_color),
            show_change_tracking: false,
            gutter_separator_color: color32(editor.gutter_separator_color),
            scrollbar_track_color: color32(editor.scrollbar_track_color),
            scrollbar_thumb_idle: color32(editor.scrollbar_thumb_idle),
            scrollbar_thumb_hover: color32(editor.scrollbar_thumb_hover),
            scrollbar_thumb_active: color32(editor.scrollbar_thumb_active),
            occurrence_highlight_color: color32(editor.occurrence_highlight_color),
            matching_bracket_color: color32(editor.matching_bracket_color),
            special_char_color: color32(editor.special_char_color),
            bookmark_marker_color: BOOKMARK_MARKER_COLOR,
            active_line_number_color: color32(editor.text_color),
            gutter_font_factor: 1.0,
            gutter_number_padding: LEGACY_GUTTER_NUMBER_PADDING,
            secondary_cursor_color: LEGACY_SECONDARY_CURSOR_COLOR,
            eol_marker: EolMarkerStyle::LegacyBadges,
            crlf_chip_bg: Color32::TRANSPARENT,
            crlf_chip_text: color32(editor.special_char_color),
        }
    }

    /// Applies the chrome-direction values to the editor theme: EOL marker
    /// style, gutter emphasis, and the themed secondary caret. Legacy themes
    /// keep every pre-redesign value.
    pub fn with_chrome(mut self, chrome: &ChromeTheme, metrics: &Metrics) -> Self {
        self.eol_marker = metrics.eol_marker;
        if metrics.style != MetricsStyle::Legacy {
            self.active_line_number_color = chrome.accent;
            self.gutter_font_factor = CHROME_GUTTER_FONT_FACTOR;
            self.gutter_number_padding = CHROME_GUTTER_NUMBER_PADDING;
            self.secondary_cursor_color = chrome.accent_dim;
            self.crlf_chip_bg = chrome.crlf_chip_bg;
            self.crlf_chip_text = chrome.crlf_chip_text;
        }
        self
    }

    /// Light theme preset (uses config-crate `builtin_light()` colors).
    pub fn light() -> Self {
        Self::from_config(
            &rust_pad_config::theme::builtin_light().editor,
            14.0,
            LEGACY_LINE_HEIGHT_FACTOR,
        )
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
        let theme = EditorTheme::from_config(&config, 20.0, LEGACY_LINE_HEIGHT_FACTOR);
        assert!((theme.font_size - 20.0).abs() < f32::EPSILON);
        assert_eq!(theme.font_id, FontId::monospace(20.0));
    }

    #[test]
    fn from_config_uses_config_colors() {
        let config = EditorColors {
            bg_color: rust_pad_config::HexColor::rgb(100, 100, 100),
            ..Default::default()
        };
        let theme = EditorTheme::from_config(&config, 14.0, LEGACY_LINE_HEIGHT_FACTOR);
        assert_eq!(theme.bg_color, Color32::from_rgb(100, 100, 100));
    }

    #[test]
    fn from_config_default_matches_dark() {
        let config = EditorColors::default();
        let from_cfg = EditorTheme::from_config(&config, 14.0, LEGACY_LINE_HEIGHT_FACTOR);
        let dark = EditorTheme::dark();
        // The default EditorColors should produce colors matching the dark theme
        assert_eq!(from_cfg.bg_color, dark.bg_color);
        assert_eq!(from_cfg.text_color, dark.text_color);
        assert_eq!(from_cfg.cursor_color, dark.cursor_color);
    }

    #[test]
    fn from_config_show_change_tracking_defaults_false() {
        let config = EditorColors::default();
        let theme = EditorTheme::from_config(&config, 14.0, LEGACY_LINE_HEIGHT_FACTOR);
        assert!(!theme.show_change_tracking);
    }

    #[test]
    fn from_config_uses_provided_line_height_factor() {
        let config = EditorColors::default();
        let theme = EditorTheme::from_config(&config, 14.0, 1.62);
        assert!((theme.line_height_factor - 1.62).abs() < f32::EPSILON);
    }

    #[test]
    fn presets_use_legacy_line_height() {
        assert!((EditorTheme::dark().line_height_factor - 1.4).abs() < f32::EPSILON);
        assert!((EditorTheme::light().line_height_factor - 1.4).abs() < f32::EPSILON);
    }

    #[test]
    fn translucent_config_colors_convert_with_straight_alpha() {
        let config = EditorColors {
            selection_color: rust_pad_config::HexColor::rgba(0x2D, 0xD4, 0xBF, 0x59),
            ..Default::default()
        };
        let theme = EditorTheme::from_config(&config, 14.0, LEGACY_LINE_HEIGHT_FACTOR);
        assert_eq!(
            theme.selection_color,
            Color32::from_rgba_unmultiplied(0x2D, 0xD4, 0xBF, 0x59)
        );
    }

    // ── with_chrome ────────────────────────────────────────────────

    #[test]
    fn from_config_defaults_keep_legacy_editor_treatment() {
        let theme =
            EditorTheme::from_config(&EditorColors::default(), 14.0, LEGACY_LINE_HEIGHT_FACTOR);
        assert_eq!(theme.active_line_number_color, theme.text_color);
        assert!((theme.gutter_font_factor - 1.0).abs() < f32::EPSILON);
        assert!((theme.gutter_number_padding - 8.0).abs() < f32::EPSILON);
        assert_eq!(
            theme.secondary_cursor_color,
            Color32::from_rgb(200, 200, 200)
        );
        assert_eq!(theme.eol_marker, EolMarkerStyle::LegacyBadges);
    }

    #[test]
    fn with_chrome_legacy_metrics_change_nothing() {
        let def = rust_pad_config::theme::builtin_dark();
        let (chrome, _) = ChromeTheme::from_definition(&def);
        let metrics = Metrics::for_definition(&def);
        let theme = EditorTheme::from_config(&def.editor, 14.0, metrics.line_height_factor)
            .with_chrome(&chrome, &metrics);
        assert_eq!(theme.active_line_number_color, theme.text_color);
        assert!((theme.gutter_font_factor - 1.0).abs() < f32::EPSILON);
        assert!((theme.gutter_number_padding - 8.0).abs() < f32::EPSILON);
        assert_eq!(
            theme.secondary_cursor_color,
            Color32::from_rgb(200, 200, 200)
        );
        assert_eq!(theme.eol_marker, EolMarkerStyle::LegacyBadges);
    }

    #[test]
    fn with_chrome_aurora_gets_return_glyph_and_accent_gutter() {
        let def = rust_pad_config::theme::aurora_dark();
        let (chrome, _) = ChromeTheme::from_definition(&def);
        let metrics = Metrics::for_definition(&def);
        let theme = EditorTheme::from_config(&def.editor, 14.0, metrics.line_height_factor)
            .with_chrome(&chrome, &metrics);
        assert_eq!(theme.eol_marker, EolMarkerStyle::ReturnGlyph);
        assert_eq!(theme.active_line_number_color, chrome.accent);
        assert_eq!(theme.secondary_cursor_color, chrome.accent_dim);
        assert!((theme.gutter_number_padding - 13.0).abs() < f32::EPSILON);
        assert!((theme.gutter_font_factor - 12.0 / 14.0).abs() < f32::EPSILON);
        assert!((theme.line_height_factor - 1.62).abs() < f32::EPSILON);
    }

    #[test]
    fn with_chrome_graphite_gets_chip_marker_and_chip_palette() {
        let def = rust_pad_config::theme::graphite_dark();
        let (chrome, _) = ChromeTheme::from_definition(&def);
        let metrics = Metrics::for_definition(&def);
        let theme = EditorTheme::from_config(&def.editor, 14.0, metrics.line_height_factor)
            .with_chrome(&chrome, &metrics);
        assert_eq!(theme.eol_marker, EolMarkerStyle::Chip);
        assert_eq!(theme.crlf_chip_bg, chrome.crlf_chip_bg);
        assert_eq!(theme.crlf_chip_text, chrome.crlf_chip_text);
        assert!((theme.line_height_factor - 1.55).abs() < f32::EPSILON);
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
