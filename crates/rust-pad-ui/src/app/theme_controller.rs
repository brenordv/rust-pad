//! Manages theme state: editor colors, syntax highlighting, zoom, and accent color.
//!
//! Encapsulates all theme-related fields that were previously spread across `App`,
//! providing a focused API for theme switching, zoom control, and visual configuration.

use egui::Color32;

use rust_pad_config::ThemeDefinition;

use crate::app::resolved_theme::{
    apply_spacing, build_visuals, ChromeTheme, Metrics, MetricsStyle,
};
use crate::editor::{EditorTheme, SyntaxHighlighter};

use super::ThemeMode;

/// Owns all theme-related state for the application.
pub struct ThemeController {
    /// The resolved editor theme (colors, font, etc.).
    pub theme: EditorTheme,
    /// The resolved chrome palette used by custom-painted UI.
    pub chrome: ChromeTheme,
    /// Metrics (radii, spacing, per-direction rendering choices).
    pub metrics: Metrics,
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
        let theme_def = Self::resolve_definition(&themes, &mut theme_mode);

        let metrics = Metrics::for_definition(&theme_def);
        let (chrome, derived) = ChromeTheme::from_definition(&theme_def);
        let editor_theme =
            EditorTheme::from_config(&theme_def.editor, font_size, metrics.line_height_factor)
                .with_chrome(&chrome, &metrics);
        Self::apply_to_context(ctx, &theme_def, &chrome, &metrics);
        Self::log_resolution(&theme_def, &metrics, derived);

        let mut syntax_highlighter = SyntaxHighlighter::new();
        syntax_highlighter.set_theme(&theme_def.syntax_theme);

        Self {
            theme: editor_theme,
            accent_color: chrome.accent,
            chrome,
            metrics,
            theme_mode,
            available_themes: themes,
            syntax_highlighter,
            default_zoom_level: zoom_level,
            max_zoom_level,
        }
    }

    /// Switches to a new theme mode and applies all theme changes.
    pub fn set_mode(&mut self, mode: ThemeMode, ctx: &egui::Context) {
        self.theme_mode = mode;
        let theme_def = Self::resolve_definition(&self.available_themes, &mut self.theme_mode);

        self.metrics = Metrics::for_definition(&theme_def);
        let (chrome, derived) = ChromeTheme::from_definition(&theme_def);
        self.theme = EditorTheme::from_config(
            &theme_def.editor,
            self.theme.font_size,
            self.metrics.line_height_factor,
        )
        .with_chrome(&chrome, &self.metrics);
        Self::apply_to_context(ctx, &theme_def, &chrome, &self.metrics);
        Self::log_resolution(&theme_def, &self.metrics, derived);
        self.accent_color = chrome.accent;
        self.chrome = chrome;
        self.syntax_highlighter.set_theme(&theme_def.syntax_theme);
    }

    /// Resolves `mode` against `themes`, falling back to System (and telling
    /// the user via the Problems log) when the named theme doesn't exist.
    fn resolve_definition(themes: &[ThemeDefinition], mode: &mut ThemeMode) -> ThemeDefinition {
        let resolved_name = mode.resolve().to_string();
        match themes.iter().find(|t| t.name == resolved_name).cloned() {
            Some(def) => def,
            None => {
                crate::problem_log::info_problem(&format!(
                    "Theme '{resolved_name}' not found, using System instead"
                ));
                *mode = ThemeMode::system();
                let fallback_name = mode.resolve().to_string();
                themes
                    .iter()
                    .find(|t| t.name == fallback_name)
                    .cloned()
                    .unwrap_or_else(rust_pad_config::theme::aurora_dark)
            }
        }
    }

    /// Pushes the resolved visuals and spacing into the egui context.
    fn apply_to_context(
        ctx: &egui::Context,
        def: &ThemeDefinition,
        chrome: &ChromeTheme,
        metrics: &Metrics,
    ) {
        ctx.set_visuals(build_visuals(def, chrome, metrics));
        apply_spacing(ctx, metrics);
    }

    /// One `info!` per resolution. Resolution only happens on theme-change
    /// events, so this line spamming the log is the tell that something
    /// started resolving per frame.
    fn log_resolution(def: &ThemeDefinition, metrics: &Metrics, derived_chrome: bool) {
        tracing::info!(
            theme = %def.name,
            style = ?metrics.style,
            derived_chrome,
            "Resolved theme"
        );
    }

    /// Whether the current theme renders with legacy (pre-redesign) metrics.
    pub fn is_legacy_style(&self) -> bool {
        self.metrics.style == MetricsStyle::Legacy
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
            chrome: ChromeTheme::default(),
            metrics: Metrics::default(),
            theme_mode: ThemeMode::dark(),
            available_themes: rust_pad_config::theme::all_builtin_themes(),
            accent_color: Color32::from_rgb(80, 180, 200),
            syntax_highlighter: SyntaxHighlighter::new(),
            default_zoom_level: 1.0,
            max_zoom_level: 15.0,
        }
    }

    #[test]
    fn resolve_definition_finds_named_theme() {
        let themes = rust_pad_config::theme::all_builtin_themes();
        let mut mode = ThemeMode("Graphite Dark".to_string());
        let def = ThemeController::resolve_definition(&themes, &mut mode);
        assert_eq!(def.name, "Graphite Dark");
        assert_eq!(mode.0, "Graphite Dark");
    }

    #[test]
    fn resolve_definition_falls_back_to_system_for_unknown_name() {
        let themes = rust_pad_config::theme::all_builtin_themes();
        let mut mode = ThemeMode("DoesNotExist".to_string());
        let def = ThemeController::resolve_definition(&themes, &mut mode);
        assert!(mode.is_system());
        assert!(
            def.name.starts_with("Aurora"),
            "System resolves to an Aurora theme"
        );
    }

    #[test]
    fn resolve_definition_last_resort_is_aurora_dark() {
        let mut mode = ThemeMode("Anything".to_string());
        let def = ThemeController::resolve_definition(&[], &mut mode);
        assert_eq!(def.name, "Aurora Dark");
    }

    #[test]
    fn legacy_style_detection_follows_metrics() {
        let mut ctrl = test_theme_ctrl();
        assert!(!ctrl.is_legacy_style());
        ctrl.metrics = Metrics::for_style(MetricsStyle::Legacy);
        assert!(ctrl.is_legacy_style());
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
        let mut doc = Document {
            zoom_level: 5.0,
            ..Default::default()
        };
        doc.zoom_level = 1.0;
        assert!((doc.zoom_level - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_doc_zoom_in_respects_custom_max() {
        let ctrl = test_theme_ctrl();
        let mut doc = Document {
            zoom_level: 1.95,
            ..Default::default()
        };
        let max = 2.0_f32;
        doc.zoom_level = (doc.zoom_level + 0.1).min(max);
        assert!((doc.zoom_level - 2.0).abs() < 0.01);
        // Verify test_theme_ctrl still constructs properly
        assert!((ctrl.default_zoom_level - 1.0).abs() < f32::EPSILON);
    }
}
