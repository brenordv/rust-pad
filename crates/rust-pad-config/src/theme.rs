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

impl EditorColors {
    /// Constructs from an ordered array of 16 colors.
    ///
    /// Order: bg, text, cursor, selection, line\_number, line\_number\_bg,
    /// current\_line\_highlight, modified\_line, saved\_line, gutter\_separator,
    /// scrollbar\_track, scrollbar\_thumb\_idle, scrollbar\_thumb\_hover,
    /// scrollbar\_thumb\_active, occurrence\_highlight, special\_char.
    fn from_palette(c: [HexColor; 16]) -> Self {
        Self {
            bg_color: c[0],
            text_color: c[1],
            cursor_color: c[2],
            selection_color: c[3],
            line_number_color: c[4],
            line_number_bg: c[5],
            current_line_highlight: c[6],
            modified_line_color: c[7],
            saved_line_color: c[8],
            gutter_separator_color: c[9],
            scrollbar_track_color: c[10],
            scrollbar_thumb_idle: c[11],
            scrollbar_thumb_hover: c[12],
            scrollbar_thumb_active: c[13],
            occurrence_highlight_color: c[14],
            special_char_color: c[15],
        }
    }
}

impl Default for EditorColors {
    fn default() -> Self {
        Self::from_palette([
            HexColor::rgb(30, 30, 30),          // bg
            HexColor::rgb(212, 212, 212),       // text
            HexColor::rgb(255, 255, 255),       // cursor
            HexColor::rgba(50, 110, 200, 100),  // selection
            HexColor::rgb(120, 120, 120),       // line_number
            HexColor::rgb(37, 37, 37),          // line_number_bg
            HexColor::rgb(45, 45, 45),          // current_line_highlight
            HexColor::rgb(230, 150, 30),        // modified_line
            HexColor::rgb(80, 180, 80),         // saved_line
            HexColor::rgb(60, 60, 60),          // gutter_separator
            HexColor::rgb(35, 35, 35),          // scrollbar_track
            HexColor::rgb(80, 80, 80),          // scrollbar_thumb_idle
            HexColor::rgb(110, 110, 110),       // scrollbar_thumb_hover
            HexColor::rgb(140, 140, 140),       // scrollbar_thumb_active
            HexColor::rgba(100, 100, 50, 80),   // occurrence_highlight
            HexColor::rgba(100, 100, 100, 180), // special_char
        ])
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

impl UiColors {
    /// Constructs from an ordered array of 9 colors.
    ///
    /// Order: panel\_fill, window\_fill, faint\_bg, extreme\_bg,
    /// noninteractive\_bg, inactive\_bg, hovered\_bg, active\_bg, accent.
    fn from_palette(c: [HexColor; 9]) -> Self {
        Self {
            panel_fill: c[0],
            window_fill: c[1],
            faint_bg_color: c[2],
            extreme_bg_color: c[3],
            widget_noninteractive_bg: c[4],
            widget_inactive_bg: c[5],
            widget_hovered_bg: c[6],
            widget_active_bg: c[7],
            accent_color: c[8],
        }
    }
}

impl Default for UiColors {
    fn default() -> Self {
        Self::from_palette([
            HexColor::rgb(43, 43, 43),   // panel_fill
            HexColor::rgb(43, 43, 43),   // window_fill
            HexColor::rgb(35, 35, 35),   // faint_bg
            HexColor::rgb(25, 25, 25),   // extreme_bg
            HexColor::rgb(43, 43, 43),   // noninteractive_bg
            HexColor::rgb(50, 50, 50),   // inactive_bg
            HexColor::rgb(60, 60, 60),   // hovered_bg
            HexColor::rgb(70, 70, 70),   // active_bg
            HexColor::rgb(80, 180, 200), // accent
        ])
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
        editor: EditorColors::from_palette([
            HexColor::rgb(255, 255, 255),       // bg
            HexColor::rgb(30, 30, 30),          // text
            HexColor::rgb(0, 0, 0),             // cursor
            HexColor::rgba(100, 150, 230, 100), // selection
            HexColor::rgb(130, 130, 130),       // line_number
            HexColor::rgb(240, 240, 240),       // line_number_bg
            HexColor::rgb(232, 242, 254),       // current_line_highlight
            HexColor::rgb(200, 120, 0),         // modified_line
            HexColor::rgb(50, 160, 50),         // saved_line
            HexColor::rgb(200, 200, 200),       // gutter_separator
            HexColor::rgb(235, 235, 235),       // scrollbar_track
            HexColor::rgb(190, 190, 190),       // scrollbar_thumb_idle
            HexColor::rgb(160, 160, 160),       // scrollbar_thumb_hover
            HexColor::rgb(130, 130, 130),       // scrollbar_thumb_active
            HexColor::rgba(255, 210, 80, 80),   // occurrence_highlight
            HexColor::rgba(170, 170, 170, 180), // special_char
        ]),
        ui: UiColors::from_palette([
            HexColor::rgb(240, 240, 240), // panel_fill
            HexColor::rgb(250, 250, 250), // window_fill
            HexColor::rgb(245, 245, 245), // faint_bg
            HexColor::rgb(255, 255, 255), // extreme_bg
            HexColor::rgb(230, 230, 230), // noninteractive_bg
            HexColor::rgb(220, 220, 220), // inactive_bg
            HexColor::rgb(210, 210, 210), // hovered_bg
            HexColor::rgb(200, 200, 200), // active_bg
            HexColor::rgb(50, 120, 200),  // accent
        ]),
    }
}

/// Sample wacky theme — deliberately clashing "retro terminal nightmare" colors.
pub fn sample_wacky() -> ThemeDefinition {
    ThemeDefinition {
        name: "Wacky".to_string(),
        dark_mode: false,
        syntax_theme: "InspiredGitHub".to_string(),
        editor: EditorColors::from_palette([
            HexColor::rgb(127, 255, 0),         // bg — chartreuse
            HexColor::rgb(0, 0, 139),           // text — dark blue
            HexColor::rgb(255, 0, 0),           // cursor — red
            HexColor::rgba(255, 140, 0, 100),   // selection — dark orange
            HexColor::rgb(255, 99, 71),         // line_number — tomato
            HexColor::rgb(0, 95, 95),           // line_number_bg — dark teal
            HexColor::rgb(154, 205, 50),        // current_line_highlight — yellow-green
            HexColor::rgb(0, 206, 209),         // modified_line — dark turquoise
            HexColor::rgb(255, 215, 0),         // saved_line — gold
            HexColor::rgb(139, 69, 19),         // gutter_separator — saddle brown
            HexColor::rgb(85, 107, 47),         // scrollbar_track — dark olive green
            HexColor::rgb(160, 82, 45),         // scrollbar_thumb_idle — sienna
            HexColor::rgb(205, 133, 63),        // scrollbar_thumb_hover — peru
            HexColor::rgb(255, 69, 0),          // scrollbar_thumb_active — orange-red
            HexColor::rgba(255, 0, 255, 80),    // occurrence_highlight — magenta
            HexColor::rgba(255, 105, 180, 180), // special_char — hot pink
        ]),
        ui: UiColors::from_palette([
            HexColor::rgb(0, 128, 128),   // panel_fill — teal
            HexColor::rgb(0, 128, 128),   // window_fill — teal
            HexColor::rgb(0, 100, 100),   // faint_bg — darker teal
            HexColor::rgb(0, 77, 77),     // extreme_bg — very dark teal
            HexColor::rgb(95, 143, 95),   // noninteractive_bg — muted green
            HexColor::rgb(107, 142, 35),  // inactive_bg — olive drab
            HexColor::rgb(189, 183, 107), // hovered_bg — dark khaki
            HexColor::rgb(218, 165, 32),  // active_bg — goldenrod
            HexColor::rgb(255, 215, 0),   // accent — gold
        ]),
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
