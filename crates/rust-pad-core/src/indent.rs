/// Indent style configuration and detection.
use serde::{Deserialize, Serialize};

/// Indentation style for a document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IndentStyle {
    /// Use N spaces for indentation.
    Spaces(usize),
    /// Use a tab character for indentation.
    Tabs,
}

impl Default for IndentStyle {
    fn default() -> Self {
        Self::Spaces(4)
    }
}

impl std::fmt::Display for IndentStyle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Spaces(n) => write!(f, "Spaces: {n}"),
            Self::Tabs => write!(f, "Tabs"),
        }
    }
}

impl IndentStyle {
    /// Returns the string to insert for one level of indentation.
    pub fn indent_text(&self) -> String {
        match self {
            Self::Spaces(n) => " ".repeat(*n),
            Self::Tabs => "\t".to_string(),
        }
    }

    /// Returns the number of characters that one indent level represents.
    pub fn indent_size(&self) -> usize {
        match self {
            Self::Spaces(n) => *n,
            Self::Tabs => 1,
        }
    }
}

/// Records an indent delta into the histogram if it's in range.
fn record_indent_delta(delta_counts: &mut [usize; 9], prev: usize, current: usize) {
    let delta = prev.abs_diff(current);
    if delta > 0 && delta < delta_counts.len() {
        delta_counts[delta] += 1;
    }
}

/// Picks the best standard indent width from the observed delta histogram.
fn best_standard_width(delta_counts: &[usize; 9]) -> IndentStyle {
    let best = [2usize, 4, 8]
        .into_iter()
        .filter(|&w| delta_counts[w] > 0)
        .max_by_key(|&w| delta_counts[w])
        .unwrap_or(4);
    IndentStyle::Spaces(best)
}

/// Detects the indentation style by scanning the first lines of text.
///
/// Compares consecutive lines to find the most common indent-level delta,
/// then picks the smallest standard width (2, 4, or 8) that explains it.
pub fn detect_indent(text: &str) -> IndentStyle {
    let mut tab_lines = 0usize;
    let mut space_lines = 0usize;
    let mut delta_counts = [0usize; 9];
    let mut prev_indent: Option<usize> = None;

    for line in text.lines().take(100) {
        let leading = line.chars().next().unwrap_or(' ');
        match leading {
            '\t' => {
                tab_lines += 1;
                prev_indent = None;
            }
            ' ' => {
                space_lines += 1;
                let spaces = line.chars().take_while(|c| *c == ' ').count();
                if let Some(prev) = prev_indent {
                    record_indent_delta(&mut delta_counts, prev, spaces);
                }
                prev_indent = Some(spaces);
            }
            _ if !line.trim().is_empty() => {
                if let Some(prev) = prev_indent {
                    record_indent_delta(&mut delta_counts, prev, 0);
                }
                prev_indent = Some(0);
            }
            _ => {}
        }
    }

    if tab_lines == 0 && space_lines == 0 {
        return IndentStyle::default();
    }
    if tab_lines > space_lines {
        return IndentStyle::Tabs;
    }

    best_standard_width(&delta_counts)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── IndentStyle basics ─────────────────────────────────────────────

    #[test]
    fn test_default_is_spaces_4() {
        assert_eq!(IndentStyle::default(), IndentStyle::Spaces(4));
    }

    #[test]
    fn test_display_spaces() {
        assert_eq!(IndentStyle::Spaces(2).to_string(), "Spaces: 2");
        assert_eq!(IndentStyle::Spaces(4).to_string(), "Spaces: 4");
        assert_eq!(IndentStyle::Spaces(8).to_string(), "Spaces: 8");
    }

    #[test]
    fn test_display_tabs() {
        assert_eq!(IndentStyle::Tabs.to_string(), "Tabs");
    }

    #[test]
    fn test_indent_text_spaces() {
        assert_eq!(IndentStyle::Spaces(2).indent_text(), "  ");
        assert_eq!(IndentStyle::Spaces(4).indent_text(), "    ");
        assert_eq!(IndentStyle::Spaces(8).indent_text(), "        ");
    }

    #[test]
    fn test_indent_text_tabs() {
        assert_eq!(IndentStyle::Tabs.indent_text(), "\t");
    }

    #[test]
    fn test_indent_size() {
        assert_eq!(IndentStyle::Spaces(2).indent_size(), 2);
        assert_eq!(IndentStyle::Spaces(4).indent_size(), 4);
        assert_eq!(IndentStyle::Tabs.indent_size(), 1);
    }

    // ── detect_indent ──────────────────────────────────────────────────

    #[test]
    fn test_detect_empty_text() {
        assert_eq!(detect_indent(""), IndentStyle::default());
    }

    #[test]
    fn test_detect_no_indentation() {
        let text = "line1\nline2\nline3\n";
        assert_eq!(detect_indent(text), IndentStyle::default());
    }

    #[test]
    fn test_detect_tabs() {
        let text = "\tline1\n\tline2\n\t\tnested\n";
        assert_eq!(detect_indent(text), IndentStyle::Tabs);
    }

    #[test]
    fn test_detect_tabs_majority() {
        // Tabs outnumber space lines → Tabs
        let text = "\ta\n\tb\n  c\n\td\n";
        assert_eq!(detect_indent(text), IndentStyle::Tabs);
    }

    #[test]
    fn test_detect_spaces_4() {
        let text = "fn main() {\n    let x = 1;\n    if true {\n        inner();\n    }\n}\n";
        assert_eq!(detect_indent(text), IndentStyle::Spaces(4));
    }

    #[test]
    fn test_detect_spaces_2() {
        let text = "function() {\n  let x = 1;\n  if (true) {\n    inner();\n  }\n}\n";
        assert_eq!(detect_indent(text), IndentStyle::Spaces(2));
    }

    #[test]
    fn test_detect_spaces_8() {
        let text = "begin\n        first\n                second\n        back\nend\n";
        assert_eq!(detect_indent(text), IndentStyle::Spaces(8));
    }

    #[test]
    fn test_detect_single_indent_level_4() {
        // All lines at same indent level — delta from 0→4 and 4→0.
        let text = "top\n    a\n    b\n    c\ntop\n";
        assert_eq!(detect_indent(text), IndentStyle::Spaces(4));
    }

    #[test]
    fn test_detect_defaults_when_ambiguous() {
        // Only odd-number indentation — no standard width matches, falls back to 4
        let text = "top\n   three\n      six\ntop\n";
        assert_eq!(detect_indent(text), IndentStyle::default());
    }
}
