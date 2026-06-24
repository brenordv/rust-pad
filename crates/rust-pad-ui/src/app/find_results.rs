//! "Find All" results panel (Notepad++ style).
//!
//! Holds the flattened list of matches produced by a Find All over the current
//! tab or all open tabs, and renders them in a dockable bottom panel. Double-
//! clicking a row asks the app to jump to that match in its tab.

/// A single match surfaced in the Find Results panel.
///
/// Char offsets (`match_start` / `match_end`) are captured at collection time;
/// they may go stale if the document is edited afterwards, so navigation clamps
/// them to the live buffer rather than trusting them blindly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FindAllResult {
    /// Index of the owning tab in `TabManager::documents`.
    pub tab_index: usize,
    /// Tab title, captured so the panel renders without touching the docs.
    pub tab_title: String,
    /// 0-indexed line where the match starts.
    pub line: usize,
    /// 0-indexed column where the match starts.
    pub col: usize,
    /// Start char index of the match in the buffer.
    pub match_start: usize,
    /// End char index of the match in the buffer (exclusive).
    pub match_end: usize,
    /// Text of the line containing the match (trailing newline stripped).
    pub line_text: String,
}

/// What the user asked the results panel to do this frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FindResultsAction {
    /// Nothing happened.
    None,
    /// Navigate to the result at this index.
    Navigate(usize),
    /// Close (hide) the panel.
    Close,
}

/// State and rendering for the bottom "Find Results" panel.
#[derive(Debug, Default)]
pub struct FindResultsPanel {
    /// Whether the panel is shown.
    pub visible: bool,
    /// The query the current results were collected for.
    query: String,
    /// Whether the query was run over all tabs (vs. the current tab only).
    all_tabs: bool,
    /// Flattened matches across the searched scope.
    results: Vec<FindAllResult>,
}

impl FindResultsPanel {
    /// Replaces the panel contents and makes it visible, even when `results`
    /// is empty (so the user sees an explicit "no results" message).
    pub fn set(&mut self, query: String, all_tabs: bool, results: Vec<FindAllResult>) {
        self.query = query;
        self.all_tabs = all_tabs;
        self.results = results;
        self.visible = true;
    }

    /// Hides the panel and drops its results.
    pub fn clear(&mut self) {
        self.visible = false;
        self.results.clear();
        self.query.clear();
    }

    /// Returns the result at `idx`, if any.
    pub fn result(&self, idx: usize) -> Option<&FindAllResult> {
        self.results.get(idx)
    }

    /// Number of results currently held.
    pub fn len(&self) -> usize {
        self.results.len()
    }

    /// Whether there are no results.
    pub fn is_empty(&self) -> bool {
        self.results.is_empty()
    }

    /// Renders the panel as a resizable bottom panel and returns the user's
    /// action for this frame. A no-op when the panel is hidden.
    pub fn show_panel(&mut self, ui: &mut egui::Ui) -> FindResultsAction {
        if !self.visible {
            return FindResultsAction::None;
        }
        let mut action = FindResultsAction::None;
        egui::Panel::bottom("find_results")
            .resizable(true)
            .default_size(160.0)
            .max_size(360.0)
            .show_inside(ui, |ui| {
                self.show_header(ui, &mut action);
                ui.separator();
                self.show_list(ui, &mut action);
            });
        action
    }

    /// Header row: summary text on the left, a close button on the right.
    fn show_header(&self, ui: &mut egui::Ui, action: &mut FindResultsAction) {
        ui.horizontal(|ui| {
            let scope = if self.all_tabs {
                "all tabs"
            } else {
                "current tab"
            };
            ui.label(format!(
                "Find results for \"{}\" — {} match{} in {scope}",
                self.query,
                self.results.len(),
                if self.results.len() == 1 { "" } else { "es" },
            ));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("✖").on_hover_text("Close results").clicked() {
                    *action = FindResultsAction::Close;
                }
            });
        });
    }

    /// Scrollable result list. Each row is a non-focusable button so it never
    /// steals keyboard focus from the editor; double-click navigates.
    fn show_list(&self, ui: &mut egui::Ui, action: &mut FindResultsAction) {
        if self.results.is_empty() {
            ui.weak("No results.");
            return;
        }
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for (idx, r) in self.results.iter().enumerate() {
                    // 1-indexed line for display; trim the rendered line so a
                    // very long line can't blow out the panel width.
                    let preview: String = r.line_text.trim().chars().take(200).collect();
                    let label = format!("{}:{}  {preview}", r.tab_title, r.line + 1);
                    let row = ui.add(
                        egui::Button::new(label)
                            .frame(false)
                            .sense(egui::Sense::CLICK),
                    );
                    if row.double_clicked() {
                        *action = FindResultsAction::Navigate(idx);
                    }
                }
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(tab_index: usize, line: usize) -> FindAllResult {
        FindAllResult {
            tab_index,
            tab_title: format!("tab{tab_index}"),
            line,
            col: 0,
            match_start: 0,
            match_end: 3,
            line_text: "the line".to_string(),
        }
    }

    #[test]
    fn set_makes_visible_and_stores_results() {
        let mut panel = FindResultsPanel::default();
        assert!(!panel.visible);
        panel.set("foo".to_string(), true, vec![sample(0, 1), sample(1, 4)]);
        assert!(panel.visible);
        assert_eq!(panel.len(), 2);
        assert!(!panel.is_empty());
        assert_eq!(panel.result(1).unwrap().tab_index, 1);
    }

    #[test]
    fn set_visible_even_with_no_results() {
        let mut panel = FindResultsPanel::default();
        panel.set("missing".to_string(), false, Vec::new());
        assert!(panel.visible, "empty results still show the panel");
        assert!(panel.is_empty());
    }

    #[test]
    fn clear_hides_and_drops_results() {
        let mut panel = FindResultsPanel::default();
        panel.set("foo".to_string(), false, vec![sample(0, 0)]);
        panel.clear();
        assert!(!panel.visible);
        assert!(panel.is_empty());
        assert!(panel.result(0).is_none());
    }
}
