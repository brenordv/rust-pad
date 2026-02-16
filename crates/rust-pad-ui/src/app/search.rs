//! Search and replace operations.
//!
//! Handles find/replace within the current tab and across all open tabs,
//! including match navigation and replace-all functionality.

use rust_pad_core::cursor::{char_to_pos, pos_to_char};
use rust_pad_core::document::Document;
use rust_pad_core::search::{SearchEngine, SearchMatch};

use crate::dialogs::{FindReplaceAction, SearchScope};

use super::App;

/// Navigates a document's cursor to select the given match.
fn navigate_to_match(doc: &mut Document, mat: &SearchMatch) {
    let pos = char_to_pos(&doc.buffer, mat.start);
    doc.cursor.clear_selection();
    doc.cursor.move_to(pos, &doc.buffer);
    doc.cursor.start_selection();
    let end_pos = char_to_pos(&doc.buffer, mat.end);
    doc.cursor.move_to(end_pos, &doc.buffer);
}

impl App {
    /// Dispatches a search action to the appropriate handler based on scope.
    pub(crate) fn handle_search_action(&mut self, action: FindReplaceAction) {
        match self.find_replace.scope {
            SearchScope::CurrentTab => self.handle_search_current_tab(action),
            SearchScope::AllTabs => self.handle_search_all_tabs(action),
        }
    }

    /// Handles search/replace within the active tab only.
    pub(crate) fn handle_search_current_tab(&mut self, action: FindReplaceAction) {
        match action {
            FindReplaceAction::Search => {
                let doc = self.tabs.active_doc_mut();
                if let Err(e) = self.find_replace.engine.find_all_versioned(
                    &doc.buffer,
                    &self.find_replace.options,
                    Some(doc.content_version),
                ) {
                    self.find_replace.status = format!("Error: {e}");
                } else {
                    let count = self.find_replace.engine.match_count();
                    if count == 0 && !self.find_replace.find_text.is_empty() {
                        self.find_replace.status = "No matches".to_string();
                    } else {
                        self.find_replace.status = format!("{count} matches");
                    }
                }
            }
            FindReplaceAction::FindNext => {
                let doc = self.tabs.active_doc_mut();
                let _ = self.find_replace.engine.find_all_versioned(
                    &doc.buffer,
                    &self.find_replace.options,
                    Some(doc.content_version),
                );

                let cursor_char = pos_to_char(&doc.buffer, doc.cursor.position).unwrap_or(0);
                if let Some(idx) = self.find_replace.engine.find_next(cursor_char) {
                    let total = self.find_replace.engine.match_count();
                    self.find_replace.status = format!("{}/{total} matches", idx + 1);
                    navigate_to_match(doc, &self.find_replace.engine.matches[idx].clone());
                } else {
                    self.find_replace.status = "No matches".to_string();
                }
            }
            FindReplaceAction::FindPrev => {
                let doc = self.tabs.active_doc_mut();
                let _ = self.find_replace.engine.find_all_versioned(
                    &doc.buffer,
                    &self.find_replace.options,
                    Some(doc.content_version),
                );

                // Use the selection start (not cursor/end) so FindPrev moves
                // backward past the currently selected match instead of re-finding it.
                let ref_pos = doc
                    .cursor
                    .selection()
                    .map(|sel| sel.start())
                    .unwrap_or(doc.cursor.position);
                let cursor_char = pos_to_char(&doc.buffer, ref_pos).unwrap_or(0);
                if let Some(idx) = self.find_replace.engine.find_prev(cursor_char) {
                    let total = self.find_replace.engine.match_count();
                    self.find_replace.status = format!("{}/{total} matches", idx + 1);
                    navigate_to_match(doc, &self.find_replace.engine.matches[idx].clone());
                } else {
                    self.find_replace.status = "No matches".to_string();
                }
            }
            FindReplaceAction::Replace => self.handle_replace_current(),
            FindReplaceAction::ReplaceAll => {
                let replacement = self.find_replace.replace_text.clone();
                let options = self.find_replace.options.clone();
                let doc = self.tabs.active_doc_mut();
                match self
                    .find_replace
                    .engine
                    .replace_all(&mut doc.buffer, &replacement, &options)
                {
                    Ok(count) => {
                        doc.modified = true;
                        self.find_replace.status = format!("Replaced {count} occurrences");
                    }
                    Err(e) => {
                        self.find_replace.status = format!("Error: {e}");
                    }
                }
            }
        }
    }

    /// Handles search/replace across all open tabs.
    pub(crate) fn handle_search_all_tabs(&mut self, action: FindReplaceAction) {
        match action {
            FindReplaceAction::Search => {
                // Count matches across all tabs
                let mut total = 0usize;
                let mut had_error = false;
                let mut error_msg = String::new();

                for doc in &self.tabs.documents {
                    let mut engine = SearchEngine::new();
                    match engine.find_all(&doc.buffer, &self.find_replace.options) {
                        Ok(()) => total += engine.match_count(),
                        Err(e) => {
                            had_error = true;
                            error_msg = format!("Error: {e}");
                        }
                    }
                }

                // Also run search on active tab to keep engine in sync for navigation
                let active_doc = self.tabs.active_doc_mut();
                let _ = self
                    .find_replace
                    .engine
                    .find_all(&active_doc.buffer, &self.find_replace.options);

                if had_error {
                    self.find_replace.status = error_msg;
                } else if total == 0 && !self.find_replace.find_text.is_empty() {
                    self.find_replace.status = "No matches in any tab".to_string();
                } else {
                    let tab_count = self.tabs.tab_count();
                    self.find_replace.status = format!("{total} matches across {tab_count} tabs");
                }
            }
            FindReplaceAction::FindNext => {
                let tab_count = self.tabs.tab_count();

                // First try to find next in active tab
                {
                    let doc = self.tabs.active_doc_mut();
                    let _ = self
                        .find_replace
                        .engine
                        .find_all(&doc.buffer, &self.find_replace.options);
                    let cursor_char = pos_to_char(&doc.buffer, doc.cursor.position).unwrap_or(0);

                    if let Some(idx) = self.find_replace.engine.find_next(cursor_char) {
                        let mat = self.find_replace.engine.matches[idx].clone();
                        if mat.start >= cursor_char || tab_count == 1 {
                            let total = self.find_replace.engine.match_count();
                            self.find_replace.status = format!("{}/{total} matches", idx + 1);
                            navigate_to_match(doc, &mat);
                            return;
                        }
                    }
                }

                // Try subsequent tabs
                if let Some((tab_idx, engine, match_idx)) = self.find_match_in_other_tabs(true, 0) {
                    let total = engine.match_count();
                    let title = self.tabs.documents[tab_idx].title.clone();
                    navigate_to_match(
                        &mut self.tabs.documents[tab_idx],
                        &engine.matches[match_idx].clone(),
                    );
                    self.tabs.active = tab_idx;
                    self.find_replace.engine = engine;
                    self.find_replace.status =
                        format!("{}/{total} matches (tab: {title})", match_idx + 1);
                    return;
                }

                self.find_replace.status = "No matches in any tab".to_string();
            }
            FindReplaceAction::FindPrev => {
                let tab_count = self.tabs.tab_count();

                // First try to find prev in active tab
                {
                    let doc = self.tabs.active_doc_mut();
                    let _ = self
                        .find_replace
                        .engine
                        .find_all(&doc.buffer, &self.find_replace.options);
                    let ref_pos = doc
                        .cursor
                        .selection()
                        .map(|sel| sel.start())
                        .unwrap_or(doc.cursor.position);
                    let cursor_char = pos_to_char(&doc.buffer, ref_pos).unwrap_or(0);

                    if let Some(idx) = self.find_replace.engine.find_prev(cursor_char) {
                        let mat = self.find_replace.engine.matches[idx].clone();
                        if mat.start < cursor_char || tab_count == 1 {
                            let total = self.find_replace.engine.match_count();
                            self.find_replace.status = format!("{}/{total} matches", idx + 1);
                            navigate_to_match(doc, &mat);
                            return;
                        }
                    }
                }

                // Try previous tabs (last match in each tab)
                if let Some((tab_idx, mut engine, match_idx)) =
                    self.find_match_in_other_tabs(false, usize::MAX)
                {
                    let total = engine.match_count();
                    let title = self.tabs.documents[tab_idx].title.clone();
                    navigate_to_match(
                        &mut self.tabs.documents[tab_idx],
                        &engine.matches[match_idx].clone(),
                    );
                    engine.current_match = Some(match_idx);
                    self.tabs.active = tab_idx;
                    self.find_replace.engine = engine;
                    self.find_replace.status =
                        format!("{}/{total} matches (tab: {title})", match_idx + 1);
                    return;
                }

                self.find_replace.status = "No matches in any tab".to_string();
            }
            FindReplaceAction::Replace => self.handle_replace_current(),
            FindReplaceAction::ReplaceAll => {
                // Replace in all tabs
                let replacement = self.find_replace.replace_text.clone();
                let options = self.find_replace.options.clone();
                let mut total_replaced = 0usize;
                let mut had_error = false;
                let mut error_msg = String::new();

                for doc in &mut self.tabs.documents {
                    let mut engine = SearchEngine::new();
                    let _ = engine.find_all(&doc.buffer, &options);
                    match engine.replace_all(&mut doc.buffer, &replacement, &options) {
                        Ok(count) => {
                            if count > 0 {
                                doc.modified = true;
                                total_replaced += count;
                            }
                        }
                        Err(e) => {
                            had_error = true;
                            error_msg = format!("Error: {e}");
                        }
                    }
                }

                // Re-sync the main engine with the active tab
                let active_doc = self.tabs.active_doc_mut();
                let _ = self
                    .find_replace
                    .engine
                    .find_all(&active_doc.buffer, &self.find_replace.options);

                if had_error {
                    self.find_replace.status = error_msg;
                } else {
                    self.find_replace.status =
                        format!("Replaced {total_replaced} occurrences across all tabs");
                }
            }
        }
    }

    /// Replaces the current match in the active tab.
    fn handle_replace_current(&mut self) {
        let doc = self.tabs.active_doc_mut();
        let replacement = self.find_replace.replace_text.clone();
        let options = self.find_replace.options.clone();
        match self
            .find_replace
            .engine
            .replace_current(&mut doc.buffer, &replacement, &options)
        {
            Ok(true) => {
                doc.modified = true;
                let count = self.find_replace.engine.match_count();
                self.find_replace.status = format!("Replaced. {count} matches remaining");
            }
            Ok(false) => {
                self.find_replace.status = "No match to replace".to_string();
            }
            Err(e) => {
                self.find_replace.status = format!("Error: {e}");
            }
        }
    }

    /// Searches other tabs for a match, returning `(tab_index, engine, match_index)`.
    ///
    /// When `forward` is true, iterates tabs forward from the active tab and returns
    /// the first match (index 0). When false, iterates backward and returns the
    /// last match. `match_hint` of 0 selects the first match, `usize::MAX` selects
    /// the last.
    fn find_match_in_other_tabs(
        &self,
        forward: bool,
        match_hint: usize,
    ) -> Option<(usize, SearchEngine, usize)> {
        let tab_count = self.tabs.tab_count();
        let start_tab = self.tabs.active;

        for offset in 1..=tab_count {
            let tab_idx = if forward {
                (start_tab + offset) % tab_count
            } else {
                (start_tab + tab_count - offset) % tab_count
            };
            let doc = &self.tabs.documents[tab_idx];
            let mut engine = SearchEngine::new();
            if engine
                .find_all(&doc.buffer, &self.find_replace.options)
                .is_ok()
                && engine.match_count() > 0
            {
                let match_idx = if match_hint == 0 {
                    0
                } else {
                    engine.match_count() - 1
                };
                return Some((tab_idx, engine, match_idx));
            }
        }
        None
    }
}
