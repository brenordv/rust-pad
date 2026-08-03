/// Theme definitions: editor colors, UI colors, chrome tokens, and built-in presets.
use serde::{Deserialize, Deserializer, Serialize};

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
    pub matching_bracket_color: HexColor,
    pub special_char_color: HexColor,
}

impl Default for EditorColors {
    fn default() -> Self {
        Self {
            bg_color: HexColor::rgb(30, 30, 30),
            text_color: HexColor::rgb(212, 212, 212),
            cursor_color: HexColor::rgb(255, 255, 255),
            selection_color: HexColor::rgba(70, 130, 220, 140),
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
            occurrence_highlight_color: HexColor::rgba(150, 140, 50, 130),
            matching_bracket_color: HexColor::rgba(190, 170, 70, 120),
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

/// Colors for the custom-painted application chrome (activity bar, tab strip,
/// status bar, breadcrumb, dialogs).
///
/// Themes that omit this block get values derived from their editor/UI colors
/// via [`ChromeColors::derive`], so pre-existing themes keep working unchanged.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ChromeColors {
    /// Fill for the menu bar, tab strip, and workspace panel.
    pub chrome_bg: HexColor,
    pub activity_bg: HexColor,
    pub status_bg: HexColor,
    /// 1px separators and panel borders.
    pub border: HexColor,
    pub text_muted: HexColor,
    pub text_faint: HexColor,
    /// Dimmed accent, e.g. open-folder glyphs.
    pub accent_dim: HexColor,
    /// Translucent accent tint for hover/selection backgrounds.
    pub accent_soft: HexColor,
    /// Text/icon color drawn on top of accent-filled surfaces.
    pub on_accent: HexColor,
    pub warn: HexColor,
    pub error: HexColor,
    pub saved: HexColor,
    pub crlf_chip_bg: HexColor,
    pub crlf_chip_text: HexColor,
    pub dialog_bg: HexColor,
    pub dialog_head: HexColor,
    pub input_bg: HexColor,
    pub button_bg: HexColor,
}

impl Default for ChromeColors {
    /// Aurora Dark values; used to fill missing fields when a theme carries a
    /// partial chrome block. Themes with no chrome block at all go through
    /// [`ChromeColors::derive`] instead.
    fn default() -> Self {
        aurora_dark_chrome()
    }
}

impl ChromeColors {
    /// Synthesizes chrome colors for themes that predate the chrome token set,
    /// using their editor and UI palettes so the derived chrome matches the
    /// theme's own look rather than any builtin's.
    pub fn derive(editor: &EditorColors, ui: &UiColors, dark_mode: bool) -> Self {
        let accent = ui.accent_color;
        let (dim_factor, soft_alpha) = if dark_mode { (0.7, 0x22) } else { (0.85, 0x1E) };
        let on_accent = if accent.luminance() > 0.5 {
            HexColor::rgb(0x08, 0x12, 0x0F)
        } else {
            HexColor::rgb(0xFF, 0xFF, 0xFF)
        };
        Self {
            chrome_bg: ui.panel_fill,
            activity_bg: ui.faint_bg_color,
            status_bg: ui.extreme_bg_color,
            border: ui.widget_active_bg,
            text_muted: editor.text_color.mix(editor.bg_color, 0.35),
            text_faint: editor.text_color.mix(editor.bg_color, 0.6),
            accent_dim: accent.scale_rgb(dim_factor),
            accent_soft: accent.with_alpha(soft_alpha),
            on_accent,
            warn: editor.modified_line_color,
            error: HexColor::rgb(0xE0, 0x44, 0x3E),
            saved: editor.saved_line_color,
            crlf_chip_bg: editor.bg_color.mix(editor.text_color, 0.1),
            crlf_chip_text: editor.special_char_color,
            dialog_bg: ui.window_fill,
            dialog_head: ui.faint_bg_color,
            input_bg: ui.extreme_bg_color,
            button_bg: ui.widget_inactive_bg,
        }
    }
}

/// Visual direction of a theme's chrome: soft/rounded vs. sharp/dense.
///
/// Determines corner radii, spacing, and per-direction rendering details.
/// Themes without a chrome block render with legacy metrics regardless of
/// this value, so the app looks unchanged for pre-existing themes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ChromeStyle {
    #[default]
    Soft,
    Sharp,
}

impl<'de> Deserialize<'de> for ChromeStyle {
    /// Unknown values fall back to `Soft` instead of failing the parse, so a
    /// hand-edited theme can never make the whole config unreadable.
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        if s.eq_ignore_ascii_case("sharp") {
            Ok(ChromeStyle::Sharp)
        } else {
            Ok(ChromeStyle::Soft)
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
    /// Chrome tokens. `None` means "derive from editor/UI colors and render
    /// with legacy metrics", the compatibility path for older themes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chrome: Option<ChromeColors>,
    #[serde(default)]
    pub chrome_style: ChromeStyle,
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
        chrome: None,
        chrome_style: ChromeStyle::Soft,
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
            matching_bracket_color: HexColor::rgba(60, 120, 200, 90),
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
        chrome: None,
        chrome_style: ChromeStyle::Soft,
    }
}

/// Sample wacky theme: deliberately clashing "retro terminal nightmare" colors.
pub fn sample_wacky() -> ThemeDefinition {
    ThemeDefinition {
        name: "Wacky".to_string(),
        dark_mode: false,
        syntax_theme: "InspiredGitHub".to_string(),
        editor: EditorColors {
            bg_color: HexColor::rgb(127, 255, 0),
            text_color: HexColor::rgb(0, 0, 139),
            cursor_color: HexColor::rgb(255, 0, 0),
            selection_color: HexColor::rgba(255, 140, 0, 100),
            line_number_color: HexColor::rgb(255, 99, 71),
            line_number_bg: HexColor::rgb(0, 95, 95),
            current_line_highlight: HexColor::rgb(154, 205, 50),
            modified_line_color: HexColor::rgb(0, 206, 209),
            saved_line_color: HexColor::rgb(255, 215, 0),
            gutter_separator_color: HexColor::rgb(139, 69, 19),
            scrollbar_track_color: HexColor::rgb(85, 107, 47),
            scrollbar_thumb_idle: HexColor::rgb(160, 82, 45),
            scrollbar_thumb_hover: HexColor::rgb(205, 133, 63),
            scrollbar_thumb_active: HexColor::rgb(255, 69, 0),
            occurrence_highlight_color: HexColor::rgba(255, 0, 255, 80),
            matching_bracket_color: HexColor::rgba(255, 215, 0, 100),
            special_char_color: HexColor::rgba(255, 105, 180, 180),
        },
        ui: UiColors {
            panel_fill: HexColor::rgb(0, 128, 128),
            window_fill: HexColor::rgb(0, 128, 128),
            faint_bg_color: HexColor::rgb(0, 100, 100),
            extreme_bg_color: HexColor::rgb(0, 77, 77),
            widget_noninteractive_bg: HexColor::rgb(95, 143, 95),
            widget_inactive_bg: HexColor::rgb(107, 142, 35),
            widget_hovered_bg: HexColor::rgb(189, 183, 107),
            widget_active_bg: HexColor::rgb(218, 165, 32),
            accent_color: HexColor::rgb(255, 215, 0),
        },
        chrome: None,
        chrome_style: ChromeStyle::Soft,
    }
}

/// Built-in "Dusk" theme: a low-glare light theme.
///
/// Uses a warm, parchment-like background (never pure white) and muted,
/// desaturated colors so it is easy on the eyes for long sessions, the
/// retina-friendly counterpart to the high-contrast `Light` theme.
pub fn builtin_dusk() -> ThemeDefinition {
    ThemeDefinition {
        name: "Dusk".to_string(),
        dark_mode: false,
        syntax_theme: "Solarized (light)".to_string(),
        editor: EditorColors {
            bg_color: HexColor::rgb(237, 233, 224),
            text_color: HexColor::rgb(60, 56, 50),
            cursor_color: HexColor::rgb(40, 38, 34),
            selection_color: HexColor::rgba(180, 165, 120, 120),
            line_number_color: HexColor::rgb(150, 142, 128),
            line_number_bg: HexColor::rgb(228, 223, 213),
            current_line_highlight: HexColor::rgb(228, 222, 208),
            modified_line_color: HexColor::rgb(190, 130, 40),
            saved_line_color: HexColor::rgb(110, 150, 90),
            gutter_separator_color: HexColor::rgb(210, 203, 190),
            scrollbar_track_color: HexColor::rgb(226, 220, 209),
            scrollbar_thumb_idle: HexColor::rgb(195, 188, 174),
            scrollbar_thumb_hover: HexColor::rgb(170, 162, 147),
            scrollbar_thumb_active: HexColor::rgb(150, 142, 126),
            occurrence_highlight_color: HexColor::rgba(200, 175, 110, 100),
            matching_bracket_color: HexColor::rgba(150, 130, 80, 120),
            special_char_color: HexColor::rgba(160, 150, 130, 170),
        },
        ui: UiColors {
            panel_fill: HexColor::rgb(228, 223, 213),
            window_fill: HexColor::rgb(233, 228, 219),
            faint_bg_color: HexColor::rgb(231, 226, 216),
            extreme_bg_color: HexColor::rgb(240, 236, 228),
            widget_noninteractive_bg: HexColor::rgb(228, 223, 213),
            widget_inactive_bg: HexColor::rgb(218, 212, 200),
            widget_hovered_bg: HexColor::rgb(208, 201, 187),
            widget_active_bg: HexColor::rgb(198, 190, 175),
            accent_color: HexColor::rgb(160, 120, 70),
        },
        chrome: None,
        chrome_style: ChromeStyle::Soft,
    }
}

fn aurora_dark_chrome() -> ChromeColors {
    ChromeColors {
        chrome_bg: HexColor::rgb(0x17, 0x1C, 0x24),
        activity_bg: HexColor::rgb(0x10, 0x14, 0x1A),
        status_bg: HexColor::rgb(0x10, 0x14, 0x1A),
        border: HexColor::rgb(0x24, 0x2C, 0x36),
        text_muted: HexColor::rgb(0x89, 0x96, 0xA3),
        text_faint: HexColor::rgb(0x5B, 0x65, 0x72),
        accent_dim: HexColor::rgb(0x20, 0x94, 0x86),
        accent_soft: HexColor::rgba(0x2D, 0xD4, 0xBF, 0x22),
        on_accent: HexColor::rgb(0x08, 0x12, 0x0F),
        warn: HexColor::rgb(0xE0, 0xA9, 0x3C),
        error: HexColor::rgb(0xE0, 0x44, 0x3E),
        saved: HexColor::rgb(0x4F, 0xB8, 0x6A),
        crlf_chip_bg: HexColor::rgb(0x1E, 0x27, 0x31),
        crlf_chip_text: HexColor::rgb(0x5E, 0x69, 0x75),
        dialog_bg: HexColor::rgb(0x1A, 0x21, 0x2A),
        dialog_head: HexColor::rgb(0x14, 0x1A, 0x22),
        input_bg: HexColor::rgb(0x0F, 0x14, 0x1A),
        button_bg: HexColor::rgb(0x23, 0x2C, 0x36),
    }
}

/// Built-in "Aurora Dark" theme: soft, rounded, blue-charcoal with a teal accent.
pub fn aurora_dark() -> ThemeDefinition {
    ThemeDefinition {
        name: "Aurora Dark".to_string(),
        dark_mode: true,
        syntax_theme: "base16-eighties.dark".to_string(),
        editor: EditorColors {
            bg_color: HexColor::rgb(0x15, 0x1A, 0x20),
            text_color: HexColor::rgb(0xC8, 0xD2, 0xDC),
            cursor_color: HexColor::rgb(0x2D, 0xD4, 0xBF),
            selection_color: HexColor::rgba(0x2D, 0xD4, 0xBF, 0x59),
            line_number_color: HexColor::rgb(0x5B, 0x65, 0x72),
            line_number_bg: HexColor::rgb(0x15, 0x1A, 0x20),
            current_line_highlight: HexColor::rgb(0x1C, 0x23, 0x2C),
            modified_line_color: HexColor::rgb(0xE0, 0xA9, 0x3C),
            saved_line_color: HexColor::rgb(0x4F, 0xB8, 0x6A),
            gutter_separator_color: HexColor::rgb(0x24, 0x2C, 0x36),
            scrollbar_track_color: HexColor::rgb(0x17, 0x1C, 0x24),
            scrollbar_thumb_idle: HexColor::rgb(0x3A, 0x43, 0x4E),
            scrollbar_thumb_hover: HexColor::rgb(0x5E, 0x6A, 0x76),
            scrollbar_thumb_active: HexColor::rgb(0x7E, 0x8B, 0x98),
            occurrence_highlight_color: HexColor::rgba(0x2D, 0xD4, 0xBF, 0x33),
            matching_bracket_color: HexColor::rgba(0x2D, 0xD4, 0xBF, 0x5A),
            special_char_color: HexColor::rgba(0x5B, 0x65, 0x72, 0xB4),
        },
        ui: UiColors {
            panel_fill: HexColor::rgb(0x17, 0x1C, 0x24),
            window_fill: HexColor::rgb(0x1A, 0x21, 0x2A),
            faint_bg_color: HexColor::rgb(0x10, 0x14, 0x1A),
            extreme_bg_color: HexColor::rgb(0x0F, 0x14, 0x1A),
            widget_noninteractive_bg: HexColor::rgb(0x17, 0x1C, 0x24),
            widget_inactive_bg: HexColor::rgb(0x23, 0x2C, 0x36),
            widget_hovered_bg: HexColor::rgb(0x2C, 0x37, 0x44),
            widget_active_bg: HexColor::rgb(0x33, 0x40, 0x4E),
            accent_color: HexColor::rgb(0x2D, 0xD4, 0xBF),
        },
        chrome: Some(aurora_dark_chrome()),
        chrome_style: ChromeStyle::Soft,
    }
}

/// Built-in "Aurora Light" theme: the light counterpart of Aurora Dark.
pub fn aurora_light() -> ThemeDefinition {
    ThemeDefinition {
        name: "Aurora Light".to_string(),
        dark_mode: false,
        syntax_theme: "InspiredGitHub".to_string(),
        editor: EditorColors {
            bg_color: HexColor::rgb(0xFB, 0xFC, 0xFE),
            text_color: HexColor::rgb(0x2B, 0x33, 0x3C),
            cursor_color: HexColor::rgb(0x12, 0x89, 0x7B),
            selection_color: HexColor::rgba(0x12, 0x89, 0x7B, 0x59),
            line_number_color: HexColor::rgb(0x9B, 0xA7, 0xB2),
            line_number_bg: HexColor::rgb(0xF3, 0xF6, 0xF9),
            current_line_highlight: HexColor::rgb(0xEA, 0xF3, 0xF1),
            modified_line_color: HexColor::rgb(0xC4, 0x88, 0x1F),
            saved_line_color: HexColor::rgb(0x3E, 0x9E, 0x5A),
            gutter_separator_color: HexColor::rgb(0xDE, 0xE5, 0xEC),
            scrollbar_track_color: HexColor::rgb(0xEF, 0xF3, 0xF7),
            scrollbar_thumb_idle: HexColor::rgb(0xCB, 0xD3, 0xDB),
            scrollbar_thumb_hover: HexColor::rgb(0xB4, 0xBE, 0xC8),
            scrollbar_thumb_active: HexColor::rgb(0x9A, 0xA6, 0xB2),
            occurrence_highlight_color: HexColor::rgba(0x12, 0x89, 0x7B, 0x33),
            matching_bracket_color: HexColor::rgba(0x12, 0x89, 0x7B, 0x5A),
            special_char_color: HexColor::rgba(0x9B, 0xA7, 0xB2, 0xB4),
        },
        ui: UiColors {
            panel_fill: HexColor::rgb(0xEF, 0xF3, 0xF7),
            window_fill: HexColor::rgb(0xFF, 0xFF, 0xFF),
            faint_bg_color: HexColor::rgb(0xE7, 0xEC, 0xF1),
            extreme_bg_color: HexColor::rgb(0xFF, 0xFF, 0xFF),
            widget_noninteractive_bg: HexColor::rgb(0xEF, 0xF3, 0xF7),
            widget_inactive_bg: HexColor::rgb(0xEA, 0xEF, 0xF4),
            widget_hovered_bg: HexColor::rgb(0xDC, 0xE1, 0xE5),
            widget_active_bg: HexColor::rgb(0xCE, 0xD2, 0xD7),
            accent_color: HexColor::rgb(0x12, 0x89, 0x7B),
        },
        chrome: Some(ChromeColors {
            chrome_bg: HexColor::rgb(0xEF, 0xF3, 0xF7),
            activity_bg: HexColor::rgb(0xE7, 0xEC, 0xF1),
            status_bg: HexColor::rgb(0xE7, 0xEC, 0xF1),
            border: HexColor::rgb(0xDE, 0xE5, 0xEC),
            text_muted: HexColor::rgb(0x69, 0x76, 0x82),
            text_faint: HexColor::rgb(0x9B, 0xA7, 0xB2),
            accent_dim: HexColor::rgb(0x0F, 0x74, 0x69),
            accent_soft: HexColor::rgba(0x12, 0x89, 0x7B, 0x1E),
            on_accent: HexColor::rgb(0xFF, 0xFF, 0xFF),
            warn: HexColor::rgb(0xC4, 0x88, 0x1F),
            error: HexColor::rgb(0xE0, 0x44, 0x3E),
            saved: HexColor::rgb(0x3E, 0x9E, 0x5A),
            crlf_chip_bg: HexColor::rgb(0xE2, 0xE8, 0xEE),
            crlf_chip_text: HexColor::rgb(0x98, 0xA4, 0xAF),
            dialog_bg: HexColor::rgb(0xFF, 0xFF, 0xFF),
            dialog_head: HexColor::rgb(0xF1, 0xF5, 0xF8),
            // Recessed well: distinct from the white dialog_bg so inputs read as
            // fields. Matches faint_bg_color for a palette-consistent step down.
            input_bg: HexColor::rgb(0xE7, 0xEC, 0xF1),
            button_bg: HexColor::rgb(0xEA, 0xEF, 0xF4),
        }),
        chrome_style: ChromeStyle::Soft,
    }
}

/// Built-in "Graphite Dark" theme: sharp, dense, near-black with a green accent.
pub fn graphite_dark() -> ThemeDefinition {
    ThemeDefinition {
        name: "Graphite Dark".to_string(),
        dark_mode: true,
        syntax_theme: "base16-eighties.dark".to_string(),
        editor: EditorColors {
            bg_color: HexColor::rgb(0x0E, 0x0F, 0x13),
            text_color: HexColor::rgb(0xD7, 0xDC, 0xE2),
            cursor_color: HexColor::rgb(0x2F, 0xE3, 0xAE),
            selection_color: HexColor::rgba(0x2F, 0xE3, 0xAE, 0x59),
            line_number_color: HexColor::rgb(0x4F, 0x55, 0x60),
            line_number_bg: HexColor::rgb(0x0E, 0x0F, 0x13),
            current_line_highlight: HexColor::rgb(0x15, 0x18, 0x1D),
            modified_line_color: HexColor::rgb(0xE0, 0xA9, 0x3C),
            saved_line_color: HexColor::rgb(0x42, 0xC0, 0x88),
            gutter_separator_color: HexColor::rgb(0x1D, 0x20, 0x27),
            scrollbar_track_color: HexColor::rgb(0x13, 0x15, 0x19),
            scrollbar_thumb_idle: HexColor::rgb(0x36, 0x3C, 0x45),
            scrollbar_thumb_hover: HexColor::rgb(0x5E, 0x65, 0x6F),
            scrollbar_thumb_active: HexColor::rgb(0x82, 0x89, 0x94),
            occurrence_highlight_color: HexColor::rgba(0x2F, 0xE3, 0xAE, 0x33),
            matching_bracket_color: HexColor::rgba(0x2F, 0xE3, 0xAE, 0x5A),
            special_char_color: HexColor::rgba(0x4F, 0x55, 0x60, 0xB4),
        },
        ui: UiColors {
            panel_fill: HexColor::rgb(0x13, 0x15, 0x19),
            window_fill: HexColor::rgb(0x14, 0x15, 0x19),
            faint_bg_color: HexColor::rgb(0x0A, 0x0B, 0x0D),
            extreme_bg_color: HexColor::rgb(0x0A, 0x0B, 0x0D),
            widget_noninteractive_bg: HexColor::rgb(0x13, 0x15, 0x19),
            widget_inactive_bg: HexColor::rgb(0x1E, 0x21, 0x28),
            widget_hovered_bg: HexColor::rgb(0x27, 0x2B, 0x34),
            widget_active_bg: HexColor::rgb(0x30, 0x35, 0x40),
            accent_color: HexColor::rgb(0x2F, 0xE3, 0xAE),
        },
        chrome: Some(ChromeColors {
            chrome_bg: HexColor::rgb(0x13, 0x15, 0x19),
            activity_bg: HexColor::rgb(0x0A, 0x0B, 0x0D),
            status_bg: HexColor::rgb(0x0A, 0x0B, 0x0D),
            border: HexColor::rgb(0x1D, 0x20, 0x27),
            text_muted: HexColor::rgb(0x7B, 0x82, 0x8D),
            text_faint: HexColor::rgb(0x4F, 0x55, 0x60),
            accent_dim: HexColor::rgb(0x21, 0x9F, 0x7A),
            accent_soft: HexColor::rgba(0x2F, 0xE3, 0xAE, 0x20),
            on_accent: HexColor::rgb(0x08, 0x12, 0x0F),
            warn: HexColor::rgb(0xE0, 0xA9, 0x3C),
            error: HexColor::rgb(0xE0, 0x44, 0x3E),
            saved: HexColor::rgb(0x42, 0xC0, 0x88),
            crlf_chip_bg: HexColor::rgb(0x19, 0x1C, 0x22),
            crlf_chip_text: HexColor::rgb(0x5B, 0x62, 0x6C),
            dialog_bg: HexColor::rgb(0x14, 0x15, 0x19),
            dialog_head: HexColor::rgb(0x0E, 0x0F, 0x13),
            input_bg: HexColor::rgb(0x0A, 0x0B, 0x0D),
            button_bg: HexColor::rgb(0x1E, 0x21, 0x28),
        }),
        chrome_style: ChromeStyle::Sharp,
    }
}

/// Built-in "Graphite Light" theme: the light counterpart of Graphite Dark.
pub fn graphite_light() -> ThemeDefinition {
    ThemeDefinition {
        name: "Graphite Light".to_string(),
        dark_mode: false,
        syntax_theme: "InspiredGitHub".to_string(),
        editor: EditorColors {
            bg_color: HexColor::rgb(0xFF, 0xFF, 0xFF),
            text_color: HexColor::rgb(0x22, 0x26, 0x2C),
            cursor_color: HexColor::rgb(0x11, 0x84, 0x66),
            selection_color: HexColor::rgba(0x11, 0x84, 0x66, 0x59),
            line_number_color: HexColor::rgb(0xA3, 0xAA, 0xB3),
            line_number_bg: HexColor::rgb(0xFA, 0xFB, 0xFC),
            current_line_highlight: HexColor::rgb(0xEF, 0xF6, 0xF3),
            modified_line_color: HexColor::rgb(0xC4, 0x88, 0x1F),
            saved_line_color: HexColor::rgb(0x2F, 0xA7, 0x72),
            gutter_separator_color: HexColor::rgb(0xE4, 0xE7, 0xEC),
            scrollbar_track_color: HexColor::rgb(0xF4, 0xF5, 0xF8),
            scrollbar_thumb_idle: HexColor::rgb(0xCD, 0xD2, 0xD9),
            scrollbar_thumb_hover: HexColor::rgb(0xB6, 0xBC, 0xC4),
            scrollbar_thumb_active: HexColor::rgb(0x9B, 0xA2, 0xAC),
            occurrence_highlight_color: HexColor::rgba(0x11, 0x84, 0x66, 0x33),
            matching_bracket_color: HexColor::rgba(0x11, 0x84, 0x66, 0x5A),
            special_char_color: HexColor::rgba(0xA3, 0xAA, 0xB3, 0xB4),
        },
        ui: UiColors {
            panel_fill: HexColor::rgb(0xF4, 0xF5, 0xF8),
            window_fill: HexColor::rgb(0xFF, 0xFF, 0xFF),
            faint_bg_color: HexColor::rgb(0xEC, 0xEE, 0xF1),
            extreme_bg_color: HexColor::rgb(0xFF, 0xFF, 0xFF),
            widget_noninteractive_bg: HexColor::rgb(0xF4, 0xF5, 0xF8),
            widget_inactive_bg: HexColor::rgb(0xEA, 0xED, 0xF1),
            widget_hovered_bg: HexColor::rgb(0xDC, 0xDF, 0xE3),
            widget_active_bg: HexColor::rgb(0xCE, 0xD1, 0xD4),
            accent_color: HexColor::rgb(0x11, 0x84, 0x66),
        },
        chrome: Some(ChromeColors {
            chrome_bg: HexColor::rgb(0xF4, 0xF5, 0xF8),
            activity_bg: HexColor::rgb(0xEC, 0xEE, 0xF1),
            status_bg: HexColor::rgb(0xEC, 0xEE, 0xF1),
            border: HexColor::rgb(0xE4, 0xE7, 0xEC),
            text_muted: HexColor::rgb(0x6D, 0x74, 0x7E),
            text_faint: HexColor::rgb(0xA3, 0xAA, 0xB3),
            accent_dim: HexColor::rgb(0x0E, 0x70, 0x57),
            accent_soft: HexColor::rgba(0x11, 0x84, 0x66, 0x1E),
            on_accent: HexColor::rgb(0xFF, 0xFF, 0xFF),
            warn: HexColor::rgb(0xC4, 0x88, 0x1F),
            error: HexColor::rgb(0xE0, 0x44, 0x3E),
            saved: HexColor::rgb(0x2F, 0xA7, 0x72),
            crlf_chip_bg: HexColor::rgb(0xEA, 0xED, 0xF1),
            crlf_chip_text: HexColor::rgb(0x9A, 0xA1, 0xAB),
            dialog_bg: HexColor::rgb(0xFF, 0xFF, 0xFF),
            dialog_head: HexColor::rgb(0xF2, 0xF4, 0xF6),
            // Recessed well: distinct from the white dialog_bg so inputs read as
            // fields. Matches faint_bg_color for a palette-consistent step down.
            input_bg: HexColor::rgb(0xEC, 0xEE, 0xF1),
            button_bg: HexColor::rgb(0xEA, 0xED, 0xF1),
        }),
        chrome_style: ChromeStyle::Sharp,
    }
}

/// Returns every built-in theme in display order.
///
/// Single source of truth for the built-in set so the default config, the
/// theme controller, and tests can't drift apart.
pub fn all_builtin_themes() -> Vec<ThemeDefinition> {
    vec![
        aurora_dark(),
        aurora_light(),
        graphite_dark(),
        graphite_light(),
        builtin_dark(),
        builtin_light(),
        builtin_dusk(),
        sample_wacky(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trips(theme: ThemeDefinition) {
        let json = serde_json::to_string_pretty(&theme).unwrap();
        let parsed: ThemeDefinition = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, theme);
    }

    #[test]
    fn test_builtin_dark_round_trip() {
        round_trips(builtin_dark());
    }

    #[test]
    fn test_builtin_light_round_trip() {
        round_trips(builtin_light());
    }

    #[test]
    fn test_builtin_dusk_round_trip() {
        round_trips(builtin_dusk());
    }

    #[test]
    fn test_aurora_and_graphite_round_trip() {
        round_trips(aurora_dark());
        round_trips(aurora_light());
        round_trips(graphite_dark());
        round_trips(graphite_light());
    }

    /// Light chrome themes must recess their input wells: `input_bg` distinct
    /// from the white `dialog_bg` (otherwise a text field renders invisibly on
    /// the dialog surface), tracking `faint_bg_color` so the recess stays on the
    /// theme's gray ramp. Dark chrome themes keep `input_bg == extreme_bg_color`
    /// so wiring the token into the text-edit fill is a no-op there, and a theme
    /// without a chrome block derives the same no-op.
    #[test]
    fn light_chrome_themes_recess_input_wells_dark_is_noop() {
        for theme in [aurora_light(), graphite_light()] {
            let chrome = theme
                .chrome
                .as_ref()
                .expect("light chrome theme has a block");
            assert_ne!(
                chrome.input_bg, chrome.dialog_bg,
                "{}: input_bg must differ from dialog_bg to read as a field",
                theme.name
            );
            assert_eq!(
                chrome.input_bg, theme.ui.faint_bg_color,
                "{}: input_bg should track faint_bg_color",
                theme.name
            );
        }
        for theme in [aurora_dark(), graphite_dark()] {
            let chrome = theme
                .chrome
                .as_ref()
                .expect("dark chrome theme has a block");
            assert_eq!(
                chrome.input_bg, theme.ui.extreme_bg_color,
                "{}: input_bg must equal extreme_bg_color so recessing inputs is a no-op",
                theme.name
            );
        }
        // A theme with no chrome block derives input_bg from extreme_bg_color,
        // so the recess helper is a no-op on legacy/derived themes too.
        let legacy = builtin_light();
        let derived = ChromeColors::derive(&legacy.editor, &legacy.ui, legacy.dark_mode);
        assert_eq!(derived.input_bg, legacy.ui.extreme_bg_color);
    }

    #[test]
    fn test_dusk_is_a_low_glare_light_theme() {
        let theme = builtin_dusk();
        assert!(!theme.dark_mode, "Dusk is a light theme");
        // Never pure white: the whole point is to avoid retina-burning glare.
        assert_ne!(theme.editor.bg_color, HexColor::rgb(255, 255, 255));
    }

    #[test]
    fn test_all_builtin_themes_contains_expected_set() {
        let names: Vec<String> = all_builtin_themes().into_iter().map(|t| t.name).collect();
        assert_eq!(
            names,
            vec![
                "Aurora Dark",
                "Aurora Light",
                "Graphite Dark",
                "Graphite Light",
                "Dark",
                "Light",
                "Dusk",
                "Wacky"
            ]
        );
    }

    #[test]
    fn test_legacy_builtins_have_no_chrome_block() {
        for name in ["Dark", "Light", "Dusk", "Wacky"] {
            let theme = all_builtin_themes()
                .into_iter()
                .find(|t| t.name == name)
                .unwrap();
            assert!(theme.chrome.is_none(), "{name} must stay on legacy path");
        }
    }

    #[test]
    fn test_new_builtins_carry_chrome_and_style() {
        let styles: Vec<(String, ChromeStyle, bool)> = all_builtin_themes()
            .into_iter()
            .filter(|t| t.chrome.is_some())
            .map(|t| (t.name, t.chrome_style, t.dark_mode))
            .collect();
        assert_eq!(
            styles,
            vec![
                ("Aurora Dark".to_string(), ChromeStyle::Soft, true),
                ("Aurora Light".to_string(), ChromeStyle::Soft, false),
                ("Graphite Dark".to_string(), ChromeStyle::Sharp, true),
                ("Graphite Light".to_string(), ChromeStyle::Sharp, false),
            ]
        );
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
        assert!(theme.chrome.is_none());
        assert_eq!(theme.chrome_style, ChromeStyle::Soft);
    }

    #[test]
    fn test_chrome_style_unknown_value_falls_back_to_soft() {
        let json = r#"{"name": "Custom", "dark_mode": true, "chrome_style": "bogus"}"#;
        let theme: ThemeDefinition = serde_json::from_str(json).unwrap();
        assert_eq!(theme.chrome_style, ChromeStyle::Soft);
    }

    #[test]
    fn test_chrome_style_sharp_parses_case_insensitively() {
        let json = r#"{"name": "Custom", "dark_mode": true, "chrome_style": "SHARP"}"#;
        let theme: ThemeDefinition = serde_json::from_str(json).unwrap();
        assert_eq!(theme.chrome_style, ChromeStyle::Sharp);
    }

    #[test]
    fn test_chrome_style_serde_round_trip() {
        for style in [ChromeStyle::Soft, ChromeStyle::Sharp] {
            let json = serde_json::to_string(&style).unwrap();
            let parsed: ChromeStyle = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, style);
        }
    }

    #[test]
    fn test_derive_uses_theme_own_palette_not_builtin_colors() {
        let wacky = sample_wacky();
        let chrome = ChromeColors::derive(&wacky.editor, &wacky.ui, wacky.dark_mode);
        assert_eq!(chrome.chrome_bg, wacky.ui.panel_fill);
        assert_eq!(chrome.warn, wacky.editor.modified_line_color);
        assert_eq!(chrome.saved, wacky.editor.saved_line_color);
        assert_ne!(chrome.chrome_bg, aurora_dark_chrome().chrome_bg);
    }

    #[test]
    fn test_derive_muted_and_faint_sit_between_text_and_bg() {
        for theme in [builtin_dark(), builtin_light(), builtin_dusk()] {
            let chrome = ChromeColors::derive(&theme.editor, &theme.ui, theme.dark_mode);
            let text_lum = theme.editor.text_color.luminance();
            let bg_lum = theme.editor.bg_color.luminance();
            let (lo, hi) = if text_lum < bg_lum {
                (text_lum, bg_lum)
            } else {
                (bg_lum, text_lum)
            };
            for derived in [chrome.text_muted, chrome.text_faint] {
                let lum = derived.luminance();
                assert!(
                    lum >= lo && lum <= hi,
                    "{}: {lum} not in {lo}..{hi}",
                    theme.name
                );
            }
        }
    }

    #[test]
    fn test_derive_on_accent_contrasts_with_accent_brightness() {
        let dark = builtin_dark();
        let chrome = ChromeColors::derive(&dark.editor, &dark.ui, true);
        assert_eq!(chrome.on_accent, HexColor::rgb(0x08, 0x12, 0x0F));

        let light = builtin_light();
        let chrome = ChromeColors::derive(&light.editor, &light.ui, false);
        assert_eq!(chrome.on_accent, HexColor::rgb(0xFF, 0xFF, 0xFF));
    }

    #[test]
    fn test_derive_accent_dim_follows_documented_factors() {
        let dark = builtin_dark();
        let chrome = ChromeColors::derive(&dark.editor, &dark.ui, true);
        assert_eq!(chrome.accent_dim, dark.ui.accent_color.scale_rgb(0.7));

        let light = builtin_light();
        let chrome = ChromeColors::derive(&light.editor, &light.ui, false);
        assert_eq!(chrome.accent_dim, light.ui.accent_color.scale_rgb(0.85));
    }

    #[test]
    fn test_theme_without_chrome_serializes_without_chrome_key() {
        let json = serde_json::to_string(&builtin_dark()).unwrap();
        assert!(!json.contains("\"chrome\""));
        assert!(json.contains("\"chrome_style\""));
    }
}
