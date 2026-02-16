/// Dialogs for Find/Replace, Go To Line, etc.
use egui::{Context, Key, Ui, Window};
use rust_pad_core::search::{SearchEngine, SearchOptions};

/// State for the Find/Replace dialog.
#[derive(Debug)]
pub struct FindReplaceDialog {
    pub visible: bool,
    pub find_text: String,
    pub replace_text: String,
    pub options: SearchOptions,
    pub engine: SearchEngine,
    /// Whether to search the current tab or all open tabs.
    pub scope: SearchScope,
    /// Status message shown in the dialog.
    pub status: String,
    /// Snapshot of options from the previous frame, used to detect checkbox changes.
    prev_options_key: String,
}

impl Default for FindReplaceDialog {
    fn default() -> Self {
        Self::new()
    }
}

impl FindReplaceDialog {
    pub fn new() -> Self {
        Self {
            visible: false,
            find_text: String::new(),
            replace_text: String::new(),
            options: SearchOptions::default(),
            engine: SearchEngine::new(),
            scope: SearchScope::default(),
            status: String::new(),
            prev_options_key: String::new(),
        }
    }

    pub fn open(&mut self) {
        self.visible = true;
    }

    pub fn close(&mut self) {
        self.visible = false;
    }

    /// Builds a key string from the current search parameters for change detection.
    fn options_key(&self) -> String {
        format!(
            "{}:{}:{}:{}:{:?}",
            self.find_text,
            self.options.case_sensitive,
            self.options.whole_word,
            self.options.use_regex,
            self.scope,
        )
    }

    /// Shows the Find/Replace dialog. Returns an action to perform, if any.
    pub fn show(&mut self, ctx: &Context) -> Option<FindReplaceAction> {
        if !self.visible {
            return None;
        }

        let mut action = None;
        let mut open = true;

        Window::new("Find and Replace")
            .collapsible(false)
            .resizable(true)
            .default_width(420.0)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing.y = 8.0;
                Self::show_find_input(ui, &mut self.find_text, &mut action);
                Self::show_replace_input(ui, &mut self.replace_text);
                ui.add_space(4.0);
                Self::show_search_options(ui, &mut self.options, &mut self.scope);
                ui.add_space(4.0);
                Self::show_action_buttons(ui, &mut action);
                if !self.status.is_empty() {
                    ui.add_space(4.0);
                    ui.label(&self.status);
                }
            });

        if !open {
            self.visible = false;
        }

        self.options.query = self.find_text.clone();
        self.detect_parameter_change(&mut action);
        action
    }

    /// Renders the find text input field.
    fn show_find_input(
        ui: &mut Ui,
        find_text: &mut String,
        action: &mut Option<FindReplaceAction>,
    ) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            ui.label("Find:      ");
            let find_response = ui.text_edit_singleline(find_text);
            if find_response.changed() {
                *action = Some(FindReplaceAction::Search);
            }
            if find_response.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                *action = Some(FindReplaceAction::FindNext);
            }
        });
    }

    /// Renders the replace text input field.
    fn show_replace_input(ui: &mut Ui, replace_text: &mut String) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            ui.label("Replace:");
            ui.text_edit_singleline(replace_text);
        });
    }

    /// Renders search option checkboxes and scope radio buttons.
    fn show_search_options(ui: &mut Ui, options: &mut SearchOptions, scope: &mut SearchScope) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 12.0;
            ui.checkbox(&mut options.case_sensitive, "Case sensitive");
            ui.checkbox(&mut options.whole_word, "Whole word");
            ui.checkbox(&mut options.use_regex, "Regex");
        });
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 12.0;
            ui.label("Scope:");
            if ui
                .radio(*scope == SearchScope::CurrentTab, "Current tab")
                .clicked()
            {
                *scope = SearchScope::CurrentTab;
            }
            if ui
                .radio(*scope == SearchScope::AllTabs, "All tabs")
                .clicked()
            {
                *scope = SearchScope::AllTabs;
            }
        });
    }

    /// Renders the Find Next / Find Prev / Replace / Replace All buttons.
    fn show_action_buttons(ui: &mut Ui, action: &mut Option<FindReplaceAction>) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            if ui.button("  Find Next  ").clicked() {
                *action = Some(FindReplaceAction::FindNext);
            }
            if ui.button("  Find Prev  ").clicked() {
                *action = Some(FindReplaceAction::FindPrev);
            }
            if ui.button("  Replace  ").clicked() {
                *action = Some(FindReplaceAction::Replace);
            }
            if ui.button("  Replace All  ").clicked() {
                *action = Some(FindReplaceAction::ReplaceAll);
            }
        });
    }

    /// Detects parameter changes and triggers a re-search if needed.
    fn detect_parameter_change(&mut self, action: &mut Option<FindReplaceAction>) {
        let current_key = self.options_key();
        if current_key != self.prev_options_key {
            self.prev_options_key = current_key;
            if action.is_none() {
                *action = Some(FindReplaceAction::Search);
            }
        }
    }
}

/// Actions that the Find/Replace dialog can request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FindReplaceAction {
    Search,
    FindNext,
    FindPrev,
    Replace,
    ReplaceAll,
}

/// Whether to search in the current tab or all open tabs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SearchScope {
    /// Search only in the active tab.
    #[default]
    CurrentTab,
    /// Search across all open tabs.
    AllTabs,
}

/// Result of parsing a "Go to" input string.
///
/// Both `line` and `column` are 0-indexed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GoToTarget {
    pub line: usize,
    pub column: usize,
}

/// Parses a go-to input string.
///
/// Accepted formats (all 1-indexed):
///   - `"42"`      → line 42, column 1
///   - `"42:10"`   → line 42, column 10
///   - `":10"`     → current line (None), column 10  — rejected (line required)
///
/// Returns `None` if the input is empty, non-numeric, or the line is out of
/// range. The column is clamped to `1..=max_col` (never rejected).
pub fn parse_goto_input(input: &str, total_lines: usize) -> Option<GoToTarget> {
    let input = input.trim();
    if input.is_empty() {
        return None;
    }

    let (line_str, col_str) = if let Some((l, c)) = input.split_once(':') {
        (l.trim(), c.trim())
    } else {
        (input, "")
    };

    let line_1based: usize = line_str.parse().ok()?;
    if line_1based < 1 || line_1based > total_lines {
        return None;
    }

    let col_1based: usize = if col_str.is_empty() {
        1
    } else {
        col_str.parse::<usize>().ok()?.max(1)
    };

    Some(GoToTarget {
        line: line_1based - 1,
        column: col_1based - 1,
    })
}

/// State for the Go To Line dialog.
#[derive(Debug)]
pub struct GoToLineDialog {
    pub visible: bool,
    pub line_text: String,
    /// When true, the text field requests focus on the next frame.
    focus_requested: bool,
}

impl Default for GoToLineDialog {
    fn default() -> Self {
        Self::new()
    }
}

impl GoToLineDialog {
    pub fn new() -> Self {
        Self {
            visible: false,
            line_text: String::new(),
            focus_requested: false,
        }
    }

    pub fn open(&mut self) {
        self.visible = true;
        self.line_text.clear();
        self.focus_requested = true;
    }

    /// Attempts to navigate: parses input, stores result, and closes the dialog.
    fn try_navigate(
        line_text: &str,
        total_lines: usize,
        result: &mut Option<GoToTarget>,
        visible: &mut bool,
    ) {
        if let Some(target) = parse_goto_input(line_text, total_lines) {
            *result = Some(target);
            *visible = false;
        }
    }

    /// Shows the Go To Line dialog. Returns a target position if confirmed.
    pub fn show(&mut self, ctx: &Context, total_lines: usize) -> Option<GoToTarget> {
        if !self.visible {
            return None;
        }

        let mut result = None;
        let mut open = true;

        Window::new("Go to Line")
            .collapsible(false)
            .resizable(false)
            .default_width(280.0)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing.y = 8.0;
                ui.label(format!("Line[:Column] (1-{total_lines}):"));
                ui.add_space(4.0);

                let response = ui.text_edit_singleline(&mut self.line_text);
                if self.focus_requested {
                    self.focus_requested = false;
                    response.request_focus();
                }

                if response.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                    Self::try_navigate(
                        &self.line_text,
                        total_lines,
                        &mut result,
                        &mut self.visible,
                    );
                }

                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    if ui.button("    Go    ").clicked() {
                        Self::try_navigate(
                            &self.line_text,
                            total_lines,
                            &mut result,
                            &mut self.visible,
                        );
                    }
                    if ui.button("  Cancel  ").clicked() {
                        self.visible = false;
                    }
                });
            });

        if !open {
            self.visible = false;
        }

        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── parse_goto_input ──────────────────────────────────────────────

    #[test]
    fn test_parse_line_only() {
        let target = parse_goto_input("42", 100).unwrap();
        assert_eq!(target.line, 41); // 0-indexed
        assert_eq!(target.column, 0);
    }

    #[test]
    fn test_parse_line_and_column() {
        let target = parse_goto_input("10:5", 100).unwrap();
        assert_eq!(target.line, 9);
        assert_eq!(target.column, 4);
    }

    #[test]
    fn test_parse_with_whitespace() {
        let target = parse_goto_input("  10 : 5  ", 100).unwrap();
        assert_eq!(target.line, 9);
        assert_eq!(target.column, 4);
    }

    #[test]
    fn test_parse_first_line() {
        let target = parse_goto_input("1", 100).unwrap();
        assert_eq!(target.line, 0);
        assert_eq!(target.column, 0);
    }

    #[test]
    fn test_parse_last_line() {
        let target = parse_goto_input("100", 100).unwrap();
        assert_eq!(target.line, 99);
        assert_eq!(target.column, 0);
    }

    #[test]
    fn test_parse_line_zero_rejected() {
        assert!(parse_goto_input("0", 100).is_none());
    }

    #[test]
    fn test_parse_line_exceeds_total() {
        assert!(parse_goto_input("101", 100).is_none());
    }

    #[test]
    fn test_parse_empty_input() {
        assert!(parse_goto_input("", 100).is_none());
    }

    #[test]
    fn test_parse_whitespace_only() {
        assert!(parse_goto_input("   ", 100).is_none());
    }

    #[test]
    fn test_parse_non_numeric() {
        assert!(parse_goto_input("abc", 100).is_none());
    }

    #[test]
    fn test_parse_column_zero_clamped_to_one() {
        // Column 0 in input is clamped to 1 (minimum), so result column is 0 (0-indexed)
        let target = parse_goto_input("5:0", 100).unwrap();
        assert_eq!(target.line, 4);
        assert_eq!(target.column, 0); // max(1,0) = 1, then 1-1 = 0
    }

    #[test]
    fn test_parse_large_column() {
        // Large column is accepted (will be clamped by cursor move_to)
        let target = parse_goto_input("1:999", 100).unwrap();
        assert_eq!(target.line, 0);
        assert_eq!(target.column, 998);
    }

    #[test]
    fn test_parse_negative_rejected() {
        // Negative numbers can't parse as usize
        assert!(parse_goto_input("-5", 100).is_none());
    }

    #[test]
    fn test_parse_colon_without_column() {
        // "10:" → empty column string → defaults to column 1
        let target = parse_goto_input("10:", 100).unwrap();
        assert_eq!(target.line, 9);
        assert_eq!(target.column, 0);
    }

    #[test]
    fn test_parse_non_numeric_column() {
        assert!(parse_goto_input("10:abc", 100).is_none());
    }

    // ── GoToLineDialog state ──────────────────────────────────────────

    #[test]
    fn test_dialog_open_clears_text() {
        let mut dialog = GoToLineDialog::new();
        dialog.line_text = "42".to_string();
        dialog.open();
        assert!(dialog.visible);
        assert!(dialog.line_text.is_empty());
    }

    #[test]
    fn test_dialog_default_not_visible() {
        let dialog = GoToLineDialog::new();
        assert!(!dialog.visible);
        assert!(dialog.line_text.is_empty());
    }
}
