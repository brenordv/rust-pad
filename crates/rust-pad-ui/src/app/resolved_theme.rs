//! Chrome theme resolution: converts config-level theme tokens into the
//! `Color32` palette, metric set, and egui `Visuals` the painters consume.
//!
//! Everything here is resolved once per theme change, never per frame.

use egui::{Color32, Vec2};

use rust_pad_config::{ChromeColors, ChromeStyle, HexColor, ThemeDefinition};

/// Fixed chrome row heights shared by every theme direction.
pub const MENU_BAR_HEIGHT: f32 = 32.0;
pub const TAB_STRIP_HEIGHT: f32 = 38.0;
pub const BREADCRUMB_HEIGHT: f32 = 29.0;
pub const STATUS_BAR_HEIGHT: f32 = 27.0;
pub const TREE_ROW_HEIGHT: f32 = 26.0;
pub const ACTIVITY_ITEM_HEIGHT: f32 = 38.0;

/// Horizontal offset of a workspace-tree row's content at `depth`.
pub fn tree_indent(depth: usize) -> f32 {
    12.0 + depth as f32 * 15.0
}

/// Converts a `HexColor` into `Color32`.
///
/// Theme tokens carry straight (unmultiplied) alpha, as authored in CSS-style
/// hex; converting them as premultiplied would render every translucent tint
/// brighter than designed.
pub fn color32(c: HexColor) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r, c.g, c.b, c.a)
}

/// The `Color32`-resolved chrome palette handed to custom painters.
#[derive(Debug, Clone, PartialEq)]
pub struct ChromeTheme {
    pub accent: Color32,
    pub chrome_bg: Color32,
    pub activity_bg: Color32,
    pub status_bg: Color32,
    pub border: Color32,
    pub text_muted: Color32,
    pub text_faint: Color32,
    pub accent_dim: Color32,
    pub accent_soft: Color32,
    pub on_accent: Color32,
    pub warn: Color32,
    pub error: Color32,
    pub saved: Color32,
    pub crlf_chip_bg: Color32,
    pub crlf_chip_text: Color32,
    pub dialog_bg: Color32,
    pub dialog_head: Color32,
    pub input_bg: Color32,
    pub button_bg: Color32,
}

impl ChromeTheme {
    /// Resolves the chrome palette for `def`.
    ///
    /// Returns the palette and whether it was derived (theme had no chrome
    /// block), so callers can log which path was taken.
    pub fn from_definition(def: &ThemeDefinition) -> (Self, bool) {
        let (colors, derived) = match &def.chrome {
            Some(chrome) => (chrome.clone(), false),
            None => (
                ChromeColors::derive(&def.editor, &def.ui, def.dark_mode),
                true,
            ),
        };
        let theme = Self {
            accent: color32(def.ui.accent_color),
            chrome_bg: color32(colors.chrome_bg),
            activity_bg: color32(colors.activity_bg),
            status_bg: color32(colors.status_bg),
            border: color32(colors.border),
            text_muted: color32(colors.text_muted),
            text_faint: color32(colors.text_faint),
            accent_dim: color32(colors.accent_dim),
            accent_soft: color32(colors.accent_soft),
            on_accent: color32(colors.on_accent),
            warn: color32(colors.warn),
            error: color32(colors.error),
            saved: color32(colors.saved),
            crlf_chip_bg: color32(colors.crlf_chip_bg),
            crlf_chip_text: color32(colors.crlf_chip_text),
            dialog_bg: color32(colors.dialog_bg),
            dialog_head: color32(colors.dialog_head),
            input_bg: color32(colors.input_bg),
            button_bg: color32(colors.button_bg),
        };
        (theme, derived)
    }
}

impl Default for ChromeTheme {
    fn default() -> Self {
        Self::from_definition(&rust_pad_config::theme::aurora_dark()).0
    }
}

/// Which metric set the chrome renders with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetricsStyle {
    Soft,
    Sharp,
    /// Pre-chrome themes; keeps today's radii, spacing, and line height so
    /// existing themes render unchanged.
    Legacy,
}

impl MetricsStyle {
    /// A theme without a chrome block always renders with legacy metrics.
    /// `chrome_style` serializes as "soft" for those themes after one config
    /// round-trip, so it must never be the deciding signal on its own.
    pub fn for_definition(def: &ThemeDefinition) -> Self {
        match (&def.chrome, def.chrome_style) {
            (None, _) => Self::Legacy,
            (Some(_), ChromeStyle::Soft) => Self::Soft,
            (Some(_), ChromeStyle::Sharp) => Self::Sharp,
        }
    }
}

/// How the active tab/tree-row/activity-item indicator is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndicatorStyle {
    /// Rounded accent-tint pill behind the item.
    TintPill,
    /// 2px accent bar on the item's leading edge.
    EdgeBar,
}

/// How line endings are marked in the editor when special chars are shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EolMarkerStyle {
    /// A single faint return glyph.
    ReturnGlyph,
    /// A compact chip carrying the line-ending name.
    Chip,
    /// The pre-redesign LF/CR/CRLF badges.
    LegacyBadges,
}

/// Numeric metrics derived from the theme's chrome style.
#[derive(Debug, Clone, PartialEq)]
pub struct Metrics {
    pub style: MetricsStyle,
    pub widget_radius: u8,
    pub window_radius: u8,
    pub menu_radius: u8,
    pub item_spacing: Vec2,
    pub button_padding: Vec2,
    pub window_margin: i8,
    pub activity_bar_width: f32,
    pub workspace_default_width: f32,
    /// Fixed height of the workspace sidebar header band.
    pub sidebar_header_height: f32,
    pub line_height_factor: f32,
    pub indicator: IndicatorStyle,
    pub eol_marker: EolMarkerStyle,
    /// Whether the status-bar cursor segment gets a filled accent background
    /// (vs. accent-colored text on the normal background).
    pub status_cursor_filled: bool,
}

impl Metrics {
    pub fn for_style(style: MetricsStyle) -> Self {
        match style {
            MetricsStyle::Soft => Self {
                style,
                widget_radius: 6,
                window_radius: 7,
                menu_radius: 6,
                item_spacing: Vec2::new(8.0, 6.0),
                button_padding: Vec2::new(11.0, 6.0),
                window_margin: 16,
                activity_bar_width: 52.0,
                workspace_default_width: 248.0,
                sidebar_header_height: 34.0,
                line_height_factor: 1.62,
                indicator: IndicatorStyle::TintPill,
                eol_marker: EolMarkerStyle::ReturnGlyph,
                status_cursor_filled: false,
            },
            MetricsStyle::Sharp => Self {
                style,
                widget_radius: 2,
                window_radius: 3,
                menu_radius: 2,
                item_spacing: Vec2::new(6.0, 4.0),
                button_padding: Vec2::new(10.0, 5.0),
                window_margin: 12,
                activity_bar_width: 46.0,
                workspace_default_width: 232.0,
                sidebar_header_height: 30.0,
                line_height_factor: 1.55,
                indicator: IndicatorStyle::EdgeBar,
                eol_marker: EolMarkerStyle::Chip,
                status_cursor_filled: true,
            },
            MetricsStyle::Legacy => Self {
                style,
                widget_radius: 4,
                window_radius: 6,
                menu_radius: 4,
                item_spacing: Vec2::new(8.0, 6.0),
                button_padding: Vec2::new(8.0, 4.0),
                window_margin: 12,
                activity_bar_width: 52.0,
                workspace_default_width: 250.0,
                sidebar_header_height: 30.0,
                line_height_factor: 1.4,
                indicator: IndicatorStyle::TintPill,
                eol_marker: EolMarkerStyle::LegacyBadges,
                status_cursor_filled: false,
            },
        }
    }

    pub fn for_definition(def: &ThemeDefinition) -> Self {
        Self::for_style(MetricsStyle::for_definition(def))
    }
}

impl Default for Metrics {
    fn default() -> Self {
        Self::for_style(MetricsStyle::Soft)
    }
}

/// Builds the egui `Visuals` for a theme.
///
/// Fill colors come from the theme's UI palette; radii, borders, and the
/// selection tint follow the resolved metrics so soft/sharp/legacy themes
/// each get their intended shape language.
pub fn build_visuals(
    def: &ThemeDefinition,
    chrome: &ChromeTheme,
    metrics: &Metrics,
) -> egui::Visuals {
    let mut visuals = if def.dark_mode {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };

    visuals.panel_fill = color32(def.ui.panel_fill);
    visuals.window_fill = color32(def.ui.window_fill);
    visuals.faint_bg_color = color32(def.ui.faint_bg_color);
    visuals.extreme_bg_color = color32(def.ui.extreme_bg_color);
    visuals.widgets.noninteractive.bg_fill = color32(def.ui.widget_noninteractive_bg);
    visuals.widgets.inactive.bg_fill = color32(def.ui.widget_inactive_bg);
    visuals.widgets.hovered.bg_fill = color32(def.ui.widget_hovered_bg);
    visuals.widgets.active.bg_fill = color32(def.ui.widget_active_bg);

    let widget_rounding = egui::CornerRadius::same(metrics.widget_radius);
    visuals.widgets.noninteractive.corner_radius = widget_rounding;
    visuals.widgets.inactive.corner_radius = widget_rounding;
    visuals.widgets.hovered.corner_radius = widget_rounding;
    visuals.widgets.active.corner_radius = widget_rounding;
    visuals.widgets.open.corner_radius = widget_rounding;
    visuals.window_corner_radius = egui::CornerRadius::same(metrics.window_radius);
    visuals.menu_corner_radius = egui::CornerRadius::same(metrics.menu_radius);

    match metrics.style {
        MetricsStyle::Legacy => {
            visuals.widgets.noninteractive.bg_stroke.width = 0.0;
            visuals.window_stroke.width = 1.0;
        }
        MetricsStyle::Soft | MetricsStyle::Sharp => {
            visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, chrome.border);
            visuals.window_stroke = egui::Stroke::new(1.0, chrome.border);
            visuals.selection.bg_fill = chrome.accent_soft;
            visuals.hyperlink_color = chrome.accent;
        }
    }

    visuals.popup_shadow = egui::Shadow {
        offset: [0, 2],
        blur: 8,
        spread: 0,
        color: Color32::from_black_alpha(40),
    };

    visuals
}

/// Applies the metric-driven global spacing to the egui style.
pub fn apply_spacing(ctx: &egui::Context, metrics: &Metrics) {
    let item_spacing = metrics.item_spacing;
    let button_padding = metrics.button_padding;
    let window_margin = metrics.window_margin;
    ctx.global_style_mut(move |style| {
        style.spacing.item_spacing = item_spacing;
        style.spacing.button_padding = button_padding;
        style.spacing.window_margin = egui::Margin::same(window_margin);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_pad_config::theme::{
        aurora_dark, aurora_light, builtin_dark, builtin_dusk, graphite_dark, graphite_light,
        sample_wacky,
    };

    #[test]
    fn color32_conversion_uses_straight_alpha() {
        let c = color32(HexColor::rgba(0x2D, 0xD4, 0xBF, 0x22));
        assert_eq!(c, Color32::from_rgba_unmultiplied(0x2D, 0xD4, 0xBF, 0x22));
        assert_ne!(c, Color32::from_rgba_premultiplied(0x2D, 0xD4, 0xBF, 0x22));
    }

    #[test]
    fn chrome_theme_uses_explicit_block_when_present() {
        let (chrome, derived) = ChromeTheme::from_definition(&aurora_dark());
        assert!(!derived);
        assert_eq!(chrome.chrome_bg, Color32::from_rgb(0x17, 0x1C, 0x24));
        assert_eq!(chrome.accent, Color32::from_rgb(0x2D, 0xD4, 0xBF));
    }

    #[test]
    fn chrome_theme_derives_for_legacy_themes() {
        let wacky = sample_wacky();
        let (chrome, derived) = ChromeTheme::from_definition(&wacky);
        assert!(derived);
        assert_eq!(chrome.chrome_bg, color32(wacky.ui.panel_fill));
    }

    #[test]
    fn metrics_style_ignores_chrome_style_without_chrome_block() {
        let mut legacy = builtin_dark();
        legacy.chrome_style = rust_pad_config::ChromeStyle::Sharp;
        assert_eq!(MetricsStyle::for_definition(&legacy), MetricsStyle::Legacy);
    }

    #[test]
    fn metrics_style_follows_chrome_style_with_chrome_block() {
        assert_eq!(
            MetricsStyle::for_definition(&aurora_light()),
            MetricsStyle::Soft
        );
        assert_eq!(
            MetricsStyle::for_definition(&graphite_dark()),
            MetricsStyle::Sharp
        );
    }

    #[test]
    fn metrics_tables_match_the_design_values() {
        let soft = Metrics::for_style(MetricsStyle::Soft);
        assert_eq!(soft.widget_radius, 6);
        assert_eq!(soft.window_radius, 7);
        assert!((soft.activity_bar_width - 52.0).abs() < f32::EPSILON);
        assert!((soft.workspace_default_width - 248.0).abs() < f32::EPSILON);
        assert!((soft.sidebar_header_height - 34.0).abs() < f32::EPSILON);
        assert!((soft.line_height_factor - 1.62).abs() < f32::EPSILON);
        assert_eq!(soft.indicator, IndicatorStyle::TintPill);
        assert_eq!(soft.eol_marker, EolMarkerStyle::ReturnGlyph);
        assert!(!soft.status_cursor_filled);

        let sharp = Metrics::for_style(MetricsStyle::Sharp);
        assert_eq!(sharp.widget_radius, 2);
        assert_eq!(sharp.window_radius, 3);
        assert!((sharp.activity_bar_width - 46.0).abs() < f32::EPSILON);
        assert!((sharp.workspace_default_width - 232.0).abs() < f32::EPSILON);
        assert!((sharp.sidebar_header_height - 30.0).abs() < f32::EPSILON);
        assert!((sharp.line_height_factor - 1.55).abs() < f32::EPSILON);
        assert_eq!(sharp.indicator, IndicatorStyle::EdgeBar);
        assert_eq!(sharp.eol_marker, EolMarkerStyle::Chip);
        assert!(sharp.status_cursor_filled);
    }

    #[test]
    fn legacy_metrics_preserve_pre_redesign_values() {
        let legacy = Metrics::for_style(MetricsStyle::Legacy);
        assert_eq!(legacy.widget_radius, 4);
        assert_eq!(legacy.window_radius, 6);
        assert_eq!(legacy.item_spacing, Vec2::new(8.0, 6.0));
        assert_eq!(legacy.button_padding, Vec2::new(8.0, 4.0));
        assert_eq!(legacy.window_margin, 12);
        assert!((legacy.sidebar_header_height - 30.0).abs() < f32::EPSILON);
        assert!((legacy.line_height_factor - 1.4).abs() < f32::EPSILON);
        assert_eq!(legacy.eol_marker, EolMarkerStyle::LegacyBadges);
    }

    #[test]
    fn build_visuals_legacy_keeps_borderless_separators() {
        let def = builtin_dusk();
        let (chrome, _) = ChromeTheme::from_definition(&def);
        let metrics = Metrics::for_definition(&def);
        let visuals = build_visuals(&def, &chrome, &metrics);
        assert!((visuals.widgets.noninteractive.bg_stroke.width - 0.0).abs() < f32::EPSILON);
        assert_eq!(
            visuals.widgets.inactive.corner_radius,
            egui::CornerRadius::same(4)
        );
    }

    #[test]
    fn build_visuals_new_directions_get_bordered_chrome_and_accent_selection() {
        for def in [aurora_dark(), graphite_light()] {
            let (chrome, _) = ChromeTheme::from_definition(&def);
            let metrics = Metrics::for_definition(&def);
            let visuals = build_visuals(&def, &chrome, &metrics);
            assert!((visuals.widgets.noninteractive.bg_stroke.width - 1.0).abs() < f32::EPSILON);
            assert_eq!(
                visuals.widgets.noninteractive.bg_stroke.color,
                chrome.border
            );
            assert_eq!(visuals.selection.bg_fill, chrome.accent_soft);
            assert_eq!(visuals.hyperlink_color, chrome.accent);
        }
    }

    #[test]
    fn build_visuals_radius_differs_between_directions() {
        let soft_def = aurora_dark();
        let (soft_chrome, _) = ChromeTheme::from_definition(&soft_def);
        let soft = build_visuals(&soft_def, &soft_chrome, &Metrics::for_definition(&soft_def));

        let sharp_def = graphite_dark();
        let (sharp_chrome, _) = ChromeTheme::from_definition(&sharp_def);
        let sharp = build_visuals(
            &sharp_def,
            &sharp_chrome,
            &Metrics::for_definition(&sharp_def),
        );

        assert_eq!(soft.window_corner_radius, egui::CornerRadius::same(7));
        assert_eq!(sharp.window_corner_radius, egui::CornerRadius::same(3));
    }

    #[test]
    fn all_four_new_themes_resolve_without_derivation() {
        for def in [
            aurora_dark(),
            aurora_light(),
            graphite_dark(),
            graphite_light(),
        ] {
            let (_, derived) = ChromeTheme::from_definition(&def);
            assert!(!derived, "{} must carry an explicit chrome block", def.name);
        }
    }

    #[test]
    fn tree_indent_follows_depth_formula() {
        assert!((tree_indent(0) - 12.0).abs() < f32::EPSILON);
        assert!((tree_indent(3) - 57.0).abs() < f32::EPSILON);
    }
}
