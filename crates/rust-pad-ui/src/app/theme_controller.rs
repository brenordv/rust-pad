//! Manages theme state: editor colors, syntax highlighting, zoom, and accent color.
//!
//! Encapsulates all theme-related fields that were previously spread across `App`,
//! providing a focused API for theme switching, zoom control, and visual configuration.

use egui::Color32;

use rust_pad_config::{ThemeDefinition, UiColors};

use crate::editor::{EditorTheme, SyntaxHighlighter};

use super::ThemeMode;

/// Owns all theme-related state for the application.
pub struct ThemeController {
    /// The resolved editor theme (colors, font, etc.).
    pub theme: EditorTheme,
    /// Which theme mode is active (System, Dark, Light, or a custom name).
    pub theme_mode: ThemeMode,
    /// All available theme definitions (built-in + user-defined).
    pub available_themes: Vec<ThemeDefinition>,
    /// Accent color used for UI highlights (e.g. active tab indicator).
    pub accent_color: Color32,
    /// Syntax highlighter wrapping syntect.
    pub syntax_highlighter: SyntaxHighlighter,
    /// Default zoom multiplier applied to newly-created documents.
    /// Per-document zoom is stored on each `Document`; this is only
    /// the initial value and the value persisted to config on exit.
    pub default_zoom_level: f32,
    /// Maximum allowed zoom level.
    pub max_zoom_level: f32,
}

impl ThemeController {
    /// Creates a new `ThemeController` from the application config and egui context.
    pub fn new(
        current_theme: &str,
        font_size: f32,
        zoom_level: f32,
        max_zoom_level: f32,
        themes: Vec<ThemeDefinition>,
        ctx: &egui::Context,
    ) -> Self {
        let mut theme_mode = ThemeMode(current_theme.to_string());
        let resolved_name = theme_mode.resolve().to_string();

        // Resolve theme definition; fall back to System if the theme doesn't exist
        let theme_def = match themes.iter().find(|t| t.name == resolved_name).cloned() {
            Some(def) => def,
            None => {
                tracing::warn!(
                    "Theme '{}' not found, falling back to System",
                    resolved_name
                );
                theme_mode = ThemeMode::system();
                let fallback_name = theme_mode.resolve().to_string();
                themes
                    .iter()
                    .find(|t| t.name == fallback_name)
                    .cloned()
                    .unwrap_or_else(rust_pad_config::theme::builtin_dark)
            }
        };

        let editor_theme = EditorTheme::from_config(&theme_def.editor, font_size);
        Self::apply_theme_visuals(ctx, &theme_def.ui, theme_def.dark_mode);
        let ac = theme_def.ui.accent_color;
        let accent_color = Color32::from_rgba_premultiplied(ac.r, ac.g, ac.b, ac.a);

        let mut syntax_highlighter = SyntaxHighlighter::new();
        syntax_highlighter.set_theme(&theme_def.syntax_theme);

        Self {
            theme: editor_theme,
            theme_mode,
            available_themes: themes,
            accent_color,
            syntax_highlighter,
            default_zoom_level: zoom_level,
            max_zoom_level,
        }
    }

    /// Switches to a new theme mode and applies all theme changes.
    pub fn set_mode(&mut self, mode: ThemeMode, ctx: &egui::Context) {
        self.theme_mode = mode;
        let resolved_name = self.theme_mode.resolve().to_string();

        // Fall back to System if the resolved theme doesn't exist
        let theme_def = match self
            .available_themes
            .iter()
            .find(|t| t.name == resolved_name)
            .cloned()
        {
            Some(def) => def,
            None => {
                tracing::warn!(
                    "Theme '{}' not found, falling back to System",
                    resolved_name
                );
                self.theme_mode = ThemeMode::system();
                let fallback_name = self.theme_mode.resolve().to_string();
                self.available_themes
                    .iter()
                    .find(|t| t.name == fallback_name)
                    .cloned()
                    .unwrap_or_else(rust_pad_config::theme::builtin_dark)
            }
        };

        self.theme = EditorTheme::from_config(&theme_def.editor, self.theme.font_size);
        Self::apply_theme_visuals(ctx, &theme_def.ui, theme_def.dark_mode);
        let ac = theme_def.ui.accent_color;
        self.accent_color = Color32::from_rgba_premultiplied(ac.r, ac.g, ac.b, ac.a);
        self.syntax_highlighter.set_theme(&theme_def.syntax_theme);
    }

    /// Applies egui visuals from config UI colors.
    pub fn apply_theme_visuals(ctx: &egui::Context, ui_colors: &UiColors, dark_mode: bool) {
        let hex = |c: rust_pad_config::HexColor| -> Color32 {
            Color32::from_rgba_premultiplied(c.r, c.g, c.b, c.a)
        };
        let mut visuals = if dark_mode {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        };

        // Fill colors
        visuals.panel_fill = hex(ui_colors.panel_fill);
        visuals.window_fill = hex(ui_colors.window_fill);
        visuals.faint_bg_color = hex(ui_colors.faint_bg_color);
        visuals.extreme_bg_color = hex(ui_colors.extreme_bg_color);
        visuals.widgets.noninteractive.bg_fill = hex(ui_colors.widget_noninteractive_bg);
        visuals.widgets.inactive.bg_fill = hex(ui_colors.widget_inactive_bg);
        visuals.widgets.hovered.bg_fill = hex(ui_colors.widget_hovered_bg);
        visuals.widgets.active.bg_fill = hex(ui_colors.widget_active_bg);

        // Widget rounding — consistent 4px on all states
        let widget_rounding = egui::CornerRadius::same(4);
        visuals.widgets.noninteractive.corner_radius = widget_rounding;
        visuals.widgets.inactive.corner_radius = widget_rounding;
        visuals.widgets.hovered.corner_radius = widget_rounding;
        visuals.widgets.active.corner_radius = widget_rounding;
        visuals.widgets.open.corner_radius = widget_rounding;

        // Window/menu rounding
        visuals.window_corner_radius = egui::CornerRadius::same(6);
        visuals.menu_corner_radius = egui::CornerRadius::same(4);

        // Clean borders
        visuals.widgets.noninteractive.bg_stroke.width = 0.0;
        visuals.window_stroke.width = 1.0;

        // Popup shadow — minimal
        visuals.popup_shadow = egui::Shadow {
            offset: [0, 2],
            blur: 8,
            spread: 0,
            color: Color32::from_black_alpha(40),
        };

        ctx.set_visuals(visuals);

        // Spacing
        ctx.global_style_mut(|style| {
            style.spacing.item_spacing = egui::Vec2::new(8.0, 6.0);
            style.spacing.button_padding = egui::Vec2::new(8.0, 4.0);
            style.spacing.window_margin = egui::Margin::same(12);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::EditorTheme;
    use rust_pad_core::document::Document;

    /// Helper: create a ThemeController for unit-testing (no egui context needed).
    fn test_theme_ctrl() -> ThemeController {
        ThemeController {
            theme: EditorTheme::default(),
            theme_mode: ThemeMode::dark(),
            available_themes: rust_pad_config::theme::all_builtin_themes(),
            accent_color: Color32::from_rgb(80, 180, 200),
            syntax_highlighter: SyntaxHighlighter::new(),
            default_zoom_level: 1.0,
            max_zoom_level: 15.0,
        }
    }

    // ── Per-document zoom (inline clamping, same logic as shortcuts/menu) ──

    #[test]
    fn test_doc_zoom_in_increments() {
        let mut doc = Document::default();
        doc.zoom_level = (doc.zoom_level + 0.1).min(15.0);
        assert!((doc.zoom_level - 1.1).abs() < 0.01);
    }

    #[test]
    fn test_doc_zoom_in_clamps_at_max() {
        let mut doc = Document::default();
        doc.zoom_level = 14.95;
        doc.zoom_level = (doc.zoom_level + 0.1).min(15.0);
        assert!((doc.zoom_level - 15.0).abs() < 0.01);
    }

    #[test]
    fn test_doc_zoom_in_does_not_exceed_max() {
        let mut doc = Document::default();
        doc.zoom_level = 15.0;
        doc.zoom_level = (doc.zoom_level + 0.1).min(15.0);
        assert!((doc.zoom_level - 15.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_doc_zoom_out_decrements() {
        let mut doc = Document::default();
        doc.zoom_level = (doc.zoom_level - 0.1).max(0.5);
        assert!((doc.zoom_level - 0.9).abs() < 0.01);
    }

    #[test]
    fn test_doc_zoom_out_clamps_at_min() {
        let mut doc = Document::default();
        doc.zoom_level = 0.55;
        doc.zoom_level = (doc.zoom_level - 0.1).max(0.5);
        assert!((doc.zoom_level - 0.5).abs() < 0.01);
    }

    #[test]
    fn test_doc_zoom_out_does_not_go_below_min() {
        let mut doc = Document::default();
        doc.zoom_level = 0.5;
        doc.zoom_level = (doc.zoom_level - 0.1).max(0.5);
        assert!((doc.zoom_level - 0.5).abs() < f32::EPSILON);
    }

    #[test]
    fn test_doc_zoom_reset() {
        let mut doc = Document::default();
        doc.zoom_level = 5.0;
        doc.zoom_level = 1.0;
        assert!((doc.zoom_level - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_doc_zoom_in_respects_custom_max() {
        let ctrl = test_theme_ctrl();
        let mut doc = Document::default();
        doc.zoom_level = 1.95;
        let max = 2.0_f32;
        doc.zoom_level = (doc.zoom_level + 0.1).min(max);
        assert!((doc.zoom_level - 2.0).abs() < 0.01);
        // Verify test_theme_ctrl still constructs properly
        assert!((ctrl.default_zoom_level - 1.0).abs() < f32::EPSILON);
    }
}
