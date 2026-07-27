//! "Find All" results panel (Notepad++ style).
//!
//! Holds the flattened list of matches produced by a Find All over the current
//! tab or all open tabs, and renders them in a dockable bottom panel. Double-
//! clicking a row asks the app to jump to that match in its tab.

/// Inner padding between a result row's rect and its text.
const ROW_TEXT_PADDING: egui::Vec2 = egui::Vec2::new(4.0, 2.0);

/// Where a Find Results row's match lives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResultSource {
    /// An open tab, by index in `TabManager::documents`.
    Tab(usize),
    /// A file on disk (from a folder search), not necessarily open.
    File(std::path::PathBuf),
}

/// A single match surfaced in the Find Results panel.
///
/// Char offsets (`match_start` / `match_end`) are captured at collection time;
/// they may go stale if the document is edited afterwards, so navigation clamps
/// them to the live buffer rather than trusting them blindly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FindAllResult {
    /// Where the match lives: an open tab or an on-disk file.
    pub source: ResultSource,
    /// Display label for the row's location (tab title or file path).
    pub location_label: String,
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
    /// Human-readable scope for the header, e.g. "all tabs", "current tab", or
    /// `folder "src"`.
    scope_label: String,
    /// Optional notice under the header (truncation / skipped-file counts).
    notice: Option<String>,
    /// Flattened matches across the searched scope.
    results: Vec<FindAllResult>,
    /// Index of the last match the user navigated to, for the footer.
    selected: Option<usize>,
}

impl FindResultsPanel {
    /// Replaces the panel contents and makes it visible, even when `results`
    /// is empty (so the user sees an explicit "no results" message).
    ///
    /// `scope_label` describes what was searched (e.g. "all tabs" or
    /// `folder "src"`); `notice` carries an optional truncation/skip note.
    pub fn set(
        &mut self,
        query: String,
        scope_label: String,
        notice: Option<String>,
        results: Vec<FindAllResult>,
    ) {
        self.query = query;
        self.scope_label = scope_label;
        self.notice = notice;
        self.results = results;
        self.selected = None;
        self.visible = true;
    }

    /// Hides the panel and drops its results.
    pub fn clear(&mut self) {
        self.visible = false;
        self.results.clear();
        self.query.clear();
        self.scope_label.clear();
        self.notice = None;
        self.selected = None;
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
    pub fn show_panel(
        &mut self,
        ui: &mut egui::Ui,
        chrome_theme: &crate::app::resolved_theme::ChromeTheme,
        metrics: &crate::app::resolved_theme::Metrics,
    ) -> FindResultsAction {
        if !self.visible {
            return FindResultsAction::None;
        }
        let mut action = FindResultsAction::None;
        let mut clicked_row = None;
        egui::Panel::bottom("find_results")
            .resizable(true)
            .default_size(160.0)
            .max_size(360.0)
            .show_inside(ui, |ui| {
                self.show_header(ui, &mut action);
                if let Some(notice) = &self.notice {
                    ui.label(
                        egui::RichText::new(notice)
                            .small()
                            .color(chrome_theme.text_muted),
                    );
                }
                ui.separator();
                // Reserve a strip at the bottom for the mini status footer.
                let footer_height = 22.0;
                let list_height = (ui.available_height() - footer_height).max(0.0);
                ui.scope(|ui| {
                    ui.set_min_height(list_height);
                    ui.set_max_height(list_height);
                    clicked_row = self.show_list(ui, chrome_theme, metrics, &mut action);
                });
                ui.separator();
                self.show_footer(ui, chrome_theme);
            });
        if let Some(idx) = clicked_row {
            self.selected = Some(idx);
        }
        if let FindResultsAction::Navigate(idx) = action {
            self.selected = Some(idx);
        }
        action
    }

    /// Text for the mini status-bar footer: `Match n/m` once the user has
    /// navigated, otherwise just the match count.
    fn footer_text(&self) -> String {
        match self.selected {
            Some(idx) if !self.results.is_empty() => {
                format!("Match {}/{}", idx + 1, self.results.len())
            }
            _ => format!(
                "{} match{}",
                self.results.len(),
                if self.results.len() == 1 { "" } else { "es" }
            ),
        }
    }

    /// Mini status-bar footer at the bottom of the panel.
    fn show_footer(
        &self,
        ui: &mut egui::Ui,
        chrome_theme: &crate::app::resolved_theme::ChromeTheme,
    ) {
        let text = self.footer_text();
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                egui::RichText::new(text)
                    .small()
                    .monospace()
                    .color(chrome_theme.text_muted),
            );
        });
    }

    /// Header row: summary text on the left, a close button on the right.
    fn show_header(&self, ui: &mut egui::Ui, action: &mut FindResultsAction) {
        ui.horizontal(|ui| {
            ui.label(format!(
                "Find results for \"{}\" — {} match{} in {}",
                self.query,
                self.results.len(),
                if self.results.len() == 1 { "" } else { "es" },
                self.scope_label,
            ));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .button(crate::icons::CLOSE)
                    .on_hover_text("Close results")
                    .clicked()
                {
                    *action = FindResultsAction::Close;
                }
            });
        });
    }

    /// Scrollable result list. Each row is a non-focusable allocated rect
    /// (bare `Sense::CLICK`) so it never steals keyboard focus from the
    /// editor. Hover paints the shared tint, the selected row the shared
    /// accent indicator; single-click selects, double-click navigates.
    ///
    /// Returns the row single-clicked this frame, if any.
    fn show_list(
        &self,
        ui: &mut egui::Ui,
        chrome_theme: &crate::app::resolved_theme::ChromeTheme,
        metrics: &crate::app::resolved_theme::Metrics,
        action: &mut FindResultsAction,
    ) -> Option<usize> {
        if self.results.is_empty() {
            ui.weak("No results.");
            return None;
        }
        let mut clicked_row = None;
        let text_color = ui.visuals().text_color();
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let row_width = ui.available_width();
                for (idx, r) in self.results.iter().enumerate() {
                    // 1-indexed line for display; trim the rendered line so a
                    // very long line can't blow out the panel width.
                    let preview: String = r.line_text.trim().chars().take(200).collect();
                    let location = format!("{}:{}", r.location_label, r.line + 1);
                    // Location prefix in accent monospace, preview in body text.
                    let mut job = egui::text::LayoutJob::default();
                    job.append(
                        &location,
                        0.0,
                        egui::TextFormat {
                            font_id: egui::FontId::monospace(12.0),
                            color: chrome_theme.accent,
                            ..Default::default()
                        },
                    );
                    job.append(
                        &preview,
                        8.0,
                        egui::TextFormat {
                            font_id: egui::FontId::proportional(13.0),
                            color: text_color,
                            ..Default::default()
                        },
                    );
                    // The old Button wrapper wrapped the job at the ui width;
                    // laying out manually keeps that behavior only if the
                    // wrap width is set explicitly.
                    job.wrap.max_width = (row_width - 2.0 * ROW_TEXT_PADDING.x).max(0.0);
                    let galley = ui.fonts_mut(|f| f.layout_job(job));
                    let row_size =
                        egui::vec2(row_width, galley.size().y + 2.0 * ROW_TEXT_PADDING.y);
                    let (rect, response) = ui.allocate_exact_size(row_size, egui::Sense::CLICK);
                    response.widget_info(|| {
                        egui::WidgetInfo::labeled(
                            egui::WidgetType::Button,
                            true,
                            format!("{location} {preview}"),
                        )
                    });
                    if self.selected == Some(idx) {
                        crate::app::chrome::accent_indicator(
                            ui.painter(),
                            rect,
                            chrome_theme,
                            metrics,
                        );
                    } else if response.hovered() {
                        crate::app::chrome::hover_tint(ui.painter(), rect, chrome_theme, metrics);
                    }
                    ui.painter()
                        .galley(rect.min + ROW_TEXT_PADDING, galley, text_color);
                    if response.double_clicked() {
                        *action = FindResultsAction::Navigate(idx);
                    } else if response.clicked() {
                        clicked_row = Some(idx);
                    }
                }
            });
        clicked_row
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(tab_index: usize, line: usize) -> FindAllResult {
        FindAllResult {
            source: ResultSource::Tab(tab_index),
            location_label: format!("tab{tab_index}"),
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
        panel.set(
            "foo".to_string(),
            "all tabs".to_string(),
            None,
            vec![sample(0, 1), sample(1, 4)],
        );
        assert!(panel.visible);
        assert_eq!(panel.len(), 2);
        assert!(!panel.is_empty());
        assert_eq!(panel.result(1).unwrap().source, ResultSource::Tab(1));
    }

    #[test]
    fn set_visible_even_with_no_results() {
        let mut panel = FindResultsPanel::default();
        panel.set(
            "missing".to_string(),
            "current tab".to_string(),
            None,
            Vec::new(),
        );
        assert!(panel.visible, "empty results still show the panel");
        assert!(panel.is_empty());
    }

    #[test]
    fn clear_hides_and_drops_results() {
        let mut panel = FindResultsPanel::default();
        panel.set(
            "foo".to_string(),
            "current tab".to_string(),
            None,
            vec![sample(0, 0)],
        );
        panel.clear();
        assert!(!panel.visible);
        assert!(panel.is_empty());
        assert!(panel.result(0).is_none());
    }

    #[test]
    fn footer_text_shows_count_before_navigation() {
        let mut panel = FindResultsPanel::default();
        panel.set(
            "foo".to_string(),
            "current tab".to_string(),
            None,
            vec![sample(0, 0), sample(0, 1)],
        );
        assert_eq!(panel.footer_text(), "2 matches");
    }

    #[test]
    fn footer_text_singular_for_one_match() {
        let mut panel = FindResultsPanel::default();
        panel.set(
            "foo".to_string(),
            "current tab".to_string(),
            None,
            vec![sample(0, 0)],
        );
        assert_eq!(panel.footer_text(), "1 match");
    }

    #[test]
    fn footer_text_shows_position_after_navigation() {
        let mut panel = FindResultsPanel::default();
        panel.set(
            "foo".to_string(),
            "current tab".to_string(),
            None,
            vec![sample(0, 0), sample(0, 1), sample(1, 2)],
        );
        panel.selected = Some(1);
        assert_eq!(panel.footer_text(), "Match 2/3");
    }

    fn list_frame(
        ctx: &egui::Context,
        panel: &FindResultsPanel,
        events: Vec<egui::Event>,
        action: &mut FindResultsAction,
    ) -> Option<usize> {
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(400.0, 300.0),
            )),
            events,
            ..Default::default()
        };
        let chrome_theme = crate::app::resolved_theme::ChromeTheme::default();
        let metrics = crate::app::resolved_theme::Metrics::default();
        let mut clicked = None;
        let _ = ctx.run_ui(raw, |ui| {
            clicked = panel.show_list(ui, &chrome_theme, &metrics, action);
        });
        clicked
    }

    fn click_events(pos: egui::Pos2, clicks: usize) -> Vec<egui::Event> {
        let mut events = vec![egui::Event::PointerMoved(pos)];
        for _ in 0..clicks {
            for pressed in [true, false] {
                events.push(egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::default(),
                });
            }
        }
        events
    }

    /// Pointer position inside the first result row: rows render from the
    /// top of the list Ui, so a point a few pixels down is inside row 0
    /// regardless of exact font metrics.
    const ROW0: egui::Pos2 = egui::Pos2::new(60.0, 8.0);

    #[test]
    fn single_click_reports_row_without_navigating() {
        let mut panel = FindResultsPanel::default();
        panel.set(
            "foo".to_string(),
            "current tab".to_string(),
            None,
            vec![sample(0, 0), sample(0, 1)],
        );
        let ctx = egui::Context::default();
        let mut action = FindResultsAction::None;

        // Layout frame so the row rects exist, hover frame, click frame.
        assert_eq!(list_frame(&ctx, &panel, Vec::new(), &mut action), None);
        list_frame(&ctx, &panel, click_events(ROW0, 0), &mut action);
        let clicked = list_frame(&ctx, &panel, click_events(ROW0, 1), &mut action);

        assert_eq!(clicked, Some(0), "single click must report the row");
        assert_eq!(
            action,
            FindResultsAction::None,
            "single click must not navigate"
        );
    }

    #[test]
    fn double_click_navigates_to_row() {
        let mut panel = FindResultsPanel::default();
        panel.set(
            "foo".to_string(),
            "current tab".to_string(),
            None,
            vec![sample(0, 0), sample(0, 1)],
        );
        let ctx = egui::Context::default();
        let mut action = FindResultsAction::None;

        assert_eq!(list_frame(&ctx, &panel, Vec::new(), &mut action), None);
        list_frame(&ctx, &panel, click_events(ROW0, 0), &mut action);
        list_frame(&ctx, &panel, click_events(ROW0, 2), &mut action);

        assert_eq!(
            action,
            FindResultsAction::Navigate(0),
            "double click must navigate to the row"
        );
    }

    #[test]
    fn set_resets_navigation_position() {
        let mut panel = FindResultsPanel::default();
        panel.set(
            "foo".to_string(),
            "current tab".to_string(),
            None,
            vec![sample(0, 0), sample(0, 1)],
        );
        panel.selected = Some(1);
        panel.set(
            "bar".to_string(),
            "current tab".to_string(),
            None,
            vec![sample(0, 0)],
        );
        assert_eq!(panel.footer_text(), "1 match");
    }
}
