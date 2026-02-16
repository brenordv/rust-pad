/// Syntax highlighting integration using syntect.
use std::path::Path;

use egui::{text::LayoutJob, Color32, FontId, TextFormat};
use syntect::easy::HighlightLines;
use syntect::highlighting::{Style, ThemeSet};
use syntect::parsing::{SyntaxReference, SyntaxSet};

/// Manages syntax highlighting state.
pub struct SyntaxHighlighter {
    syntax_set: SyntaxSet,
    theme_set: ThemeSet,
    current_theme: String,
}

impl SyntaxHighlighter {
    /// Creates a new syntax highlighter with default syntax definitions and themes.
    pub fn new() -> Self {
        Self {
            syntax_set: SyntaxSet::load_defaults_newlines(),
            theme_set: ThemeSet::load_defaults(),
            current_theme: "base16-eighties.dark".to_string(),
        }
    }

    /// Detects the syntax for a file based on its extension.
    pub fn detect_syntax(&self, file_path: Option<&Path>) -> &SyntaxReference {
        if let Some(path) = file_path {
            if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                if let Some(syntax) = self.syntax_set.find_syntax_by_extension(ext) {
                    return syntax;
                }
            }
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if let Some(syntax) = self.syntax_set.find_syntax_by_extension(name) {
                    return syntax;
                }
            }
        }
        self.syntax_set.find_syntax_plain_text()
    }

    /// Highlights a single line of text and returns an egui LayoutJob.
    pub fn highlight_line(
        &self,
        line: &str,
        _syntax: &SyntaxReference,
        highlighter: &mut HighlightLines<'_>,
        font_id: &FontId,
    ) -> LayoutJob {
        let mut job = LayoutJob::default();

        let ranges = highlighter
            .highlight_line(line, &self.syntax_set)
            .unwrap_or_default();

        for (style, text) in ranges {
            let color = syntect_color_to_egui(style);
            job.append(
                text,
                0.0,
                TextFormat {
                    font_id: font_id.clone(),
                    color,
                    ..Default::default()
                },
            );
        }

        job
    }

    /// Creates a new highlighter instance for line-by-line highlighting.
    pub fn create_highlighter(&self, syntax: &SyntaxReference) -> Option<HighlightLines<'_>> {
        let theme = self.theme_set.themes.get(&self.current_theme)?;
        Some(HighlightLines::new(syntax, theme))
    }

    /// Returns the syntax set reference.
    #[allow(dead_code)]
    pub fn syntax_set(&self) -> &SyntaxSet {
        &self.syntax_set
    }

    /// Returns a list of available theme names.
    #[allow(dead_code)]
    pub fn available_themes(&self) -> Vec<&str> {
        self.theme_set.themes.keys().map(|s| s.as_str()).collect()
    }

    /// Sets the current theme.
    pub fn set_theme(&mut self, theme_name: &str) {
        if self.theme_set.themes.contains_key(theme_name) {
            self.current_theme = theme_name.to_string();
        }
    }

    /// Returns the current theme name.
    #[allow(dead_code)]
    pub fn current_theme(&self) -> &str {
        &self.current_theme
    }
}

/// Converts a syntect color to an egui Color32.
fn syntect_color_to_egui(style: Style) -> Color32 {
    Color32::from_rgba_unmultiplied(
        style.foreground.r,
        style.foreground.g,
        style.foreground.b,
        style.foreground.a,
    )
}
