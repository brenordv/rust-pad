/// Theme definitions: editor colors, UI colors, and built-in theme presets.
use serde::{Deserialize, Serialize};

use crate::color::HexColor;

/// Colors for the editor widget (gutter, text area, scrollbars).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct EditorColors {
    pub bg_color: HexColor,
    pub text_color: HexColor,
    pub cursor_color: HexColor,
    pub selection_color: HexColor,
    pub line_number_color: HexColor,
    pub line_number_bg: HexColor,
    pub current_line_highlight: HexColor,
    pub modified_line_color: HexColor,
    pub saved_line_color: HexColor,
    pub gutter_separator_color: HexColor,
    pub scrollbar_track_color: HexColor,
    pub scrollbar_thumb_idle: HexColor,
    pub scrollbar_thumb_hover: HexColor,
    pub scrollbar_thumb_active: HexColor,
    pub occurrence_highlight_color: HexColor,
    pub special_char_color: HexColor,
}

impl Default for EditorColors {
    fn default() -> Self {
        Self {
            bg_color: HexColor::rgb(30, 30, 30),
            text_color: HexColor::rgb(212, 212, 212),
            cursor_color: HexColor::rgb(255, 255, 255),
            selection_color: HexColor::rgba(50, 110, 200, 100),
            line_number_color: HexColor::rgb(120, 120, 120),
            line_number_bg: HexColor::rgb(37, 37, 37),
            current_line_highlight: HexColor::rgb(45, 45, 45),
            modified_line_color: HexColor::rgb(230, 150, 30),
            saved_line_color: HexColor::rgb(80, 180, 80),
            gutter_separator_color: HexColor::rgb(60, 60, 60),
            scrollbar_track_color: HexColor::rgb(35, 35, 35),
            scrollbar_thumb_idle: HexColor::rgb(80, 80, 80),
            scrollbar_thumb_hover: HexColor::rgb(110, 110, 110),
            scrollbar_thumb_active: HexColor::rgb(140, 140, 140),
            occurrence_highlight_color: HexColor::rgba(100, 100, 50, 80),
            special_char_color: HexColor::rgba(100, 100, 100, 180),
        }
    }
}

/// Colors for egui UI elements (panels, widgets, backgrounds).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct UiColors {
    pub panel_fill: HexColor,
    pub window_fill: HexColor,
    pub faint_bg_color: HexColor,
    pub extreme_bg_color: HexColor,
    pub widget_noninteractive_bg: HexColor,
    pub widget_inactive_bg: HexColor,
    pub widget_hovered_bg: HexColor,
    pub widget_active_bg: HexColor,
    pub accent_color: HexColor,
}

impl Default for UiColors {
    fn default() -> Self {
        Self {
            panel_fill: HexColor::rgb(43, 43, 43),
            window_fill: HexColor::rgb(43, 43, 43),
            faint_bg_color: HexColor::rgb(35, 35, 35),
            extreme_bg_color: HexColor::rgb(25, 25, 25),
            widget_noninteractive_bg: HexColor::rgb(43, 43, 43),
            widget_inactive_bg: HexColor::rgb(50, 50, 50),
            widget_hovered_bg: HexColor::rgb(60, 60, 60),
            widget_active_bg: HexColor::rgb(70, 70, 70),
            accent_color: HexColor::rgb(80, 180, 200),
        }
    }
}

/// A complete theme definition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThemeDefinition {
    pub name: String,
    pub dark_mode: bool,
    #[serde(default = "default_syntax_theme")]
    pub syntax_theme: String,
    #[serde(default)]
    pub editor: EditorColors,
    #[serde(default)]
    pub ui: UiColors,
}

fn default_syntax_theme() -> String {
    "base16-eighties.dark".to_string()
}

/// Built-in dark theme.
pub fn builtin_dark() -> ThemeDefinition {
    ThemeDefinition {
        name: "Dark".to_string(),
        dark_mode: true,
        syntax_theme: "base16-eighties.dark".to_string(),
        editor: EditorColors::default(),
        ui: UiColors::default(),
    }
}

/// Built-in light theme.
pub fn builtin_light() -> ThemeDefinition {
    ThemeDefinition {
        name: "Light".to_string(),
        dark_mode: false,
        syntax_theme: "InspiredGitHub".to_string(),
        editor: EditorColors {
            bg_color: HexColor::rgb(255, 255, 255),
            text_color: HexColor::rgb(30, 30, 30),
            cursor_color: HexColor::rgb(0, 0, 0),
            selection_color: HexColor::rgba(100, 150, 230, 100),
            line_number_color: HexColor::rgb(130, 130, 130),
            line_number_bg: HexColor::rgb(240, 240, 240),
            current_line_highlight: HexColor::rgb(232, 242, 254),
            modified_line_color: HexColor::rgb(200, 120, 0),
            saved_line_color: HexColor::rgb(50, 160, 50),
            gutter_separator_color: HexColor::rgb(200, 200, 200),
            scrollbar_track_color: HexColor::rgb(235, 235, 235),
            scrollbar_thumb_idle: HexColor::rgb(190, 190, 190),
            scrollbar_thumb_hover: HexColor::rgb(160, 160, 160),
            scrollbar_thumb_active: HexColor::rgb(130, 130, 130),
            occurrence_highlight_color: HexColor::rgba(255, 210, 80, 80),
            special_char_color: HexColor::rgba(170, 170, 170, 180),
        },
        ui: UiColors {
            panel_fill: HexColor::rgb(240, 240, 240),
            window_fill: HexColor::rgb(250, 250, 250),
            faint_bg_color: HexColor::rgb(245, 245, 245),
            extreme_bg_color: HexColor::rgb(255, 255, 255),
            widget_noninteractive_bg: HexColor::rgb(230, 230, 230),
            widget_inactive_bg: HexColor::rgb(220, 220, 220),
            widget_hovered_bg: HexColor::rgb(210, 210, 210),
            widget_active_bg: HexColor::rgb(200, 200, 200),
            accent_color: HexColor::rgb(50, 120, 200),
        },
    }
}

/// Sample wacky theme — deliberately clashing "retro terminal nightmare" colors.
pub fn sample_wacky() -> ThemeDefinition {
    ThemeDefinition {
        name: "Wacky".to_string(),
        dark_mode: false,
        syntax_theme: "InspiredGitHub".to_string(),
        editor: EditorColors {
            bg_color: HexColor::rgb(127, 255, 0),              // chartreuse
            text_color: HexColor::rgb(0, 0, 139),              // dark blue
            cursor_color: HexColor::rgb(255, 0, 0),            // red
            selection_color: HexColor::rgba(255, 140, 0, 100), // dark orange
            line_number_color: HexColor::rgb(255, 99, 71),     // tomato
            line_number_bg: HexColor::rgb(0, 95, 95),          // dark teal
            current_line_highlight: HexColor::rgb(154, 205, 50), // yellow-green
            modified_line_color: HexColor::rgb(0, 206, 209),   // dark turquoise
            saved_line_color: HexColor::rgb(255, 215, 0),      // gold
            gutter_separator_color: HexColor::rgb(139, 69, 19), // saddle brown
            scrollbar_track_color: HexColor::rgb(85, 107, 47), // dark olive green
            scrollbar_thumb_idle: HexColor::rgb(160, 82, 45),  // sienna
            scrollbar_thumb_hover: HexColor::rgb(205, 133, 63), // peru
            scrollbar_thumb_active: HexColor::rgb(255, 69, 0), // orange-red
            occurrence_highlight_color: HexColor::rgba(255, 0, 255, 80), // magenta
            special_char_color: HexColor::rgba(255, 105, 180, 180), // hot pink
        },
        ui: UiColors {
            panel_fill: HexColor::rgb(0, 128, 128),               // teal
            window_fill: HexColor::rgb(0, 128, 128),              // teal
            faint_bg_color: HexColor::rgb(0, 100, 100),           // darker teal
            extreme_bg_color: HexColor::rgb(0, 77, 77),           // very dark teal
            widget_noninteractive_bg: HexColor::rgb(95, 143, 95), // muted green
            widget_inactive_bg: HexColor::rgb(107, 142, 35),      // olive drab
            widget_hovered_bg: HexColor::rgb(189, 183, 107),      // dark khaki
            widget_active_bg: HexColor::rgb(218, 165, 32),        // goldenrod
            accent_color: HexColor::rgb(255, 215, 0),             // gold
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builtin_dark_round_trip() {
        let theme = builtin_dark();
        let json = serde_json::to_string_pretty(&theme).unwrap();
        let parsed: ThemeDefinition = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, theme);
    }

    #[test]
    fn test_builtin_light_round_trip() {
        let theme = builtin_light();
        let json = serde_json::to_string_pretty(&theme).unwrap();
        let parsed: ThemeDefinition = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, theme);
    }

    #[test]
    fn test_partial_editor_colors_fills_defaults() {
        let json = r##"{"bg_color": "#FF0000"}"##;
        let colors: EditorColors = serde_json::from_str(json).unwrap();
        assert_eq!(colors.bg_color, HexColor::rgb(255, 0, 0));
        // Rest should be defaults
        assert_eq!(colors.text_color, EditorColors::default().text_color);
    }

    #[test]
    fn test_partial_theme_definition() {
        let json = r#"{"name": "Custom", "dark_mode": true}"#;
        let theme: ThemeDefinition = serde_json::from_str(json).unwrap();
        assert_eq!(theme.name, "Custom");
        assert!(theme.dark_mode);
        assert_eq!(theme.editor, EditorColors::default());
        assert_eq!(theme.ui, UiColors::default());
    }
}
