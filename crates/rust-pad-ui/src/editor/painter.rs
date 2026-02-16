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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_creates_valid_highlighter() {
        let hl = SyntaxHighlighter::new();
        assert_eq!(hl.current_theme(), "base16-eighties.dark");
    }

    #[test]
    fn detect_syntax_rust_extension() {
        let hl = SyntaxHighlighter::new();
        let syntax = hl.detect_syntax(Some(Path::new("main.rs")));
        assert_eq!(syntax.name, "Rust");
    }

    #[test]
    fn detect_syntax_python_extension() {
        let hl = SyntaxHighlighter::new();
        let syntax = hl.detect_syntax(Some(Path::new("script.py")));
        assert_eq!(syntax.name, "Python");
    }

    #[test]
    fn detect_syntax_javascript_extension() {
        let hl = SyntaxHighlighter::new();
        let syntax = hl.detect_syntax(Some(Path::new("app.js")));
        assert_eq!(syntax.name, "JavaScript");
    }

    #[test]
    fn detect_syntax_no_path_returns_plain_text() {
        let hl = SyntaxHighlighter::new();
        let syntax = hl.detect_syntax(None);
        assert_eq!(syntax.name, "Plain Text");
    }

    #[test]
    fn detect_syntax_unknown_extension_returns_plain_text() {
        let hl = SyntaxHighlighter::new();
        let syntax = hl.detect_syntax(Some(Path::new("file.xyz_unknown")));
        assert_eq!(syntax.name, "Plain Text");
    }

    #[test]
    fn detect_syntax_makefile_by_filename() {
        let hl = SyntaxHighlighter::new();
        let syntax = hl.detect_syntax(Some(Path::new("Makefile")));
        assert_eq!(syntax.name, "Makefile");
    }

    #[test]
    fn create_highlighter_returns_some() {
        let hl = SyntaxHighlighter::new();
        let syntax = hl.detect_syntax(Some(Path::new("test.rs")));
        assert!(hl.create_highlighter(syntax).is_some());
    }

    #[test]
    fn set_theme_valid_name() {
        let mut hl = SyntaxHighlighter::new();
        let themes = hl.available_themes();
        assert!(!themes.is_empty());
        // Set to a different valid theme
        let other: String = themes
            .iter()
            .find(|&&t| t != hl.current_theme())
            .unwrap()
            .to_string();
        hl.set_theme(&other);
        assert_eq!(hl.current_theme(), other);
    }

    #[test]
    fn set_theme_invalid_name_is_noop() {
        let mut hl = SyntaxHighlighter::new();
        let original = hl.current_theme().to_string();
        hl.set_theme("nonexistent-theme-name");
        assert_eq!(hl.current_theme(), original);
    }

    #[test]
    fn available_themes_not_empty() {
        let hl = SyntaxHighlighter::new();
        let themes = hl.available_themes();
        assert!(!themes.is_empty());
        assert!(themes.contains(&"base16-eighties.dark"));
    }

    #[test]
    fn highlight_line_produces_non_empty_job() {
        let hl = SyntaxHighlighter::new();
        let syntax = hl.detect_syntax(Some(Path::new("test.rs")));
        let mut highlighter = hl.create_highlighter(syntax).unwrap();
        let font_id = FontId::monospace(14.0);

        let job = hl.highlight_line("fn main() {}", syntax, &mut highlighter, &font_id);
        assert!(!job.text.is_empty());
    }

    #[test]
    fn highlight_line_empty_string() {
        let hl = SyntaxHighlighter::new();
        let syntax = hl.detect_syntax(Some(Path::new("test.rs")));
        let mut highlighter = hl.create_highlighter(syntax).unwrap();
        let font_id = FontId::monospace(14.0);

        let job = hl.highlight_line("", syntax, &mut highlighter, &font_id);
        assert!(job.text.is_empty());
    }

    #[test]
    fn syntect_color_conversion() {
        let style = Style {
            foreground: syntect::highlighting::Color {
                r: 255,
                g: 128,
                b: 64,
                a: 200,
            },
            ..Default::default()
        };
        let color = syntect_color_to_egui(style);
        assert_eq!(color, Color32::from_rgba_unmultiplied(255, 128, 64, 200));
    }
}
