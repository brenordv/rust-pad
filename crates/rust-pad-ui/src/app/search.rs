//! Search and replace operations.
//!
//! Handles find/replace within the current tab and across all open tabs,
//! including match navigation and replace-all functionality.

use std::path::{Path, PathBuf};

use rust_pad_core::buffer::TextBuffer;
use rust_pad_core::cursor::{char_to_pos, pos_to_char};
use rust_pad_core::document::Document;
use rust_pad_core::search::{
    DocRevision, FolderSearchLimits, FolderSearchOutcome, SearchEngine, SearchMatch, SearchOptions,
};

use crate::dialogs::{FindReplaceAction, SearchScope};
use crate::io_worker::IoRequest;

use super::find_results::{FindAllResult, ResultSource};
use super::App;

/// A folder-search hit whose file must be opened before the editor can jump to
/// the match. Held while the async read is in flight.
#[derive(Debug, Clone)]
pub(crate) struct PendingFindNav {
    /// Canonical path of the file to open.
    pub path: PathBuf,
    /// Match char offsets and line, applied once the file loads.
    pub start: usize,
    pub end: usize,
    pub line: usize,
}

/// Runs a one-shot search over `buffer`, returning every match (empty on error
/// or empty query). Shared by Find All so each tab is searched identically
/// without duplicating the engine setup.
fn matches_for(buffer: &TextBuffer, options: &SearchOptions) -> Vec<SearchMatch> {
    let mut engine = SearchEngine::new();
    match engine.find_all(buffer, options) {
        Ok(()) => engine.matches,
        Err(_) => Vec::new(),
    }
}

/// Returns the text of `line` with any trailing line break stripped, or an
/// empty string when the line is out of range.
fn line_text_for(buffer: &TextBuffer, line: usize) -> String {
    match buffer.line(line) {
        Ok(slice) => slice.to_string().trim_end_matches(['\n', '\r']).to_string(),
        Err(_) => String::new(),
    }
}

/// Navigates a document's cursor to select the given match.
fn navigate_to_match(doc: &mut Document, mat: &SearchMatch) {
    let pos = char_to_pos(&doc.buffer, mat.start);
    doc.cursor.clear_selection();
    doc.cursor.move_to(pos, &doc.buffer);
    doc.cursor.start_selection();
    let end_pos = char_to_pos(&doc.buffer, mat.end);
    doc.cursor.move_to(end_pos, &doc.buffer);
    doc.scroll_to_cursor = true;
}

/// Builds a user-facing notice about truncation and skipped files, or `None`
/// when the folder search covered everything (so partial results are never
/// presented as complete).
fn folder_search_notice(outcome: &FolderSearchOutcome) -> Option<String> {
    let skipped = outcome.skipped_too_large
        + outcome.skipped_binary
        + outcome.skipped_out_of_root
        + outcome.unreadable;
    if !outcome.truncated && skipped == 0 {
        return None;
    }
    let mut parts = Vec::new();
    if outcome.truncated {
        parts.push("results truncated at the search limit; narrow your search".to_string());
    }
    if skipped > 0 {
        parts.push(format!(
            "{skipped} file(s) skipped (binary, too large, or outside the folder)"
        ));
    }
    Some(parts.join("; "))
}

impl App {
    /// Dispatches a search action to the appropriate handler based on scope.
    pub(crate) fn handle_search_action(&mut self, action: FindReplaceAction) {
        // The folder picker is a UI action, not a query: it opens a dialog, is
        // never recorded in history, and never reaches the scope handlers.
        if action == FindReplaceAction::ChooseFolder {
            self.choose_search_folder();
            return;
        }
        // Record search history for actionable operations (not on every keystroke).
        match action {
            FindReplaceAction::FindNext
            | FindReplaceAction::FindPrev
            | FindReplaceAction::Replace
            | FindReplaceAction::ReplaceAll
            | FindReplaceAction::FindAll => {
                self.find_replace.record_search();
            }
            FindReplaceAction::Search | FindReplaceAction::ChooseFolder => {}
        }
        // Find All spans the chosen scope itself, so it is handled before the
        // per-scope split below.
        if action == FindReplaceAction::FindAll {
            if self.find_replace.scope == SearchScope::Folder {
                self.dispatch_folder_search();
            } else {
                self.collect_find_all();
            }
            return;
        }
        match self.find_replace.scope {
            SearchScope::CurrentTab => self.handle_search_current_tab(action),
            SearchScope::AllTabs => self.handle_search_all_tabs(action),
            // Folder scope only supports Find All (async); incremental actions
            // point the user at the button rather than searching a stale tab.
            SearchScope::Folder => {
                self.find_replace.status = "Press Find All to search the folder".to_string();
            }
        }
    }

    /// Primes a folder search for `folder`: stores it and switches to Folder
    /// scope. A pure state transition (no picker), so it is unit-testable
    /// without opening a GUI dialog.
    pub(crate) fn set_search_folder(&mut self, folder: PathBuf) {
        tracing::debug!(folder = ?folder, "search folder chosen");
        self.find_replace.folder_path = Some(folder);
        self.find_replace.scope = SearchScope::Folder;
        self.find_replace.status.clear();
    }

    /// Opens a native folder picker and, if the user chooses a folder, primes a
    /// folder search for it. Blocking on the UI thread, matching the workspace
    /// folder picker (`add_folder_to_workspace`). Seeded from the current search
    /// folder when it still exists, else the configured default directory. Not
    /// called from tests (it would open a real dialog).
    fn choose_search_folder(&mut self) {
        let mut dialog = rfd::FileDialog::new();
        let seed = self
            .find_replace
            .folder_path
            .as_deref()
            .filter(|p| p.is_dir())
            .map(std::path::Path::to_path_buf)
            .or_else(|| self.file_dialog.resolve_directory());
        if let Some(dir) = seed {
            dialog = dialog.set_directory(&dir);
        }
        if let Some(folder) = dialog.pick_folder() {
            self.set_search_folder(folder);
        }
    }

    /// Dispatches an off-thread folder search for the primed folder + query.
    ///
    /// The folder is canonicalized here (UI thread); the worker re-checks
    /// containment per file. A generation token lets a later search supersede
    /// this one when its result arrives out of order.
    fn dispatch_folder_search(&mut self) {
        let Some(folder) = self.find_replace.folder_path.clone() else {
            self.find_replace.status = "Choose a folder to search".to_string();
            return;
        };
        let query = self.find_replace.find_text.clone();
        if query.trim().is_empty() {
            self.find_replace.status = "Enter text to search".to_string();
            return;
        }
        let canonical = match std::fs::canonicalize(&folder) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!(error = %e, "[FS01] Folder search refused: canonicalize");
                crate::problem_log::warn_problem(
                    "[FS01] Folder search refused: the folder could not be resolved.",
                );
                self.find_replace.status = "Folder is not accessible".to_string();
                return;
            }
        };

        let mut limits = FolderSearchLimits::default();
        if let Some(max) = self.max_file_size_bytes {
            limits.max_file_size_bytes = max;
        }
        // Sync the query into the options: `options.query` is only refreshed
        // inside `FindReplaceDialog::show`, but folder searches are dispatched
        // from the sidebar/tab menus outside that render pass.
        let mut options = self.find_replace.options.clone();
        options.query = query.clone();
        self.folder_search_generation = self.folder_search_generation.wrapping_add(1);
        let generation = self.folder_search_generation;
        tracing::info!(generation, "folder search dispatched");
        tracing::debug!(root = ?canonical, query = %query, "folder search dispatched");
        self.find_replace.status = "Searching folder…".to_string();
        self.io_worker.send(IoRequest::SearchFolder {
            generation,
            root: canonical,
            options,
            limits,
        });
    }

    /// Populates the results panel from a completed folder search. Stale
    /// results (superseded generation) are dropped by the caller.
    pub(crate) fn populate_folder_search_results(
        &mut self,
        root: &Path,
        outcome: FolderSearchOutcome,
    ) {
        let query = self.find_replace.find_text.clone();
        let root_name = root
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| root.display().to_string());
        let scope_label = format!("folder \"{root_name}\"");

        let results: Vec<FindAllResult> = outcome
            .hits
            .iter()
            .map(|h| {
                let label = h
                    .path
                    .strip_prefix(root)
                    .unwrap_or(&h.path)
                    .to_string_lossy()
                    .into_owned();
                FindAllResult {
                    source: ResultSource::File(h.path.clone()),
                    location_label: label,
                    line: h.line,
                    col: h.col,
                    match_start: h.match_start,
                    match_end: h.match_end,
                    line_text: h.line_text.clone(),
                }
            })
            .collect();

        let count = results.len();
        let notice = folder_search_notice(&outcome);
        self.find_results.set(query, scope_label, notice, results);
        self.find_replace.status = if count == 0 {
            "No matches".to_string()
        } else {
            format!("{count} matches")
        };
        tracing::info!(
            files_visited = outcome.files_visited,
            hits = count,
            skipped_too_large = outcome.skipped_too_large,
            skipped_binary = outcome.skipped_binary,
            skipped_out_of_root = outcome.skipped_out_of_root,
            unreadable = outcome.unreadable,
            truncated = outcome.truncated,
            "folder search complete"
        );
    }

    /// Collects every match in the active scope (current tab or all open tabs)
    /// into the Find Results panel and shows it.
    pub(crate) fn collect_find_all(&mut self) {
        let options = self.find_replace.options.clone();
        let query = self.find_replace.find_text.clone();
        let all_tabs = self.find_replace.scope == SearchScope::AllTabs;

        let mut results = Vec::new();
        if !query.trim().is_empty() {
            let tab_indices: Vec<usize> = if all_tabs {
                (0..self.tabs.tab_count()).collect()
            } else {
                vec![self.tabs.active]
            };
            for tab_index in tab_indices {
                let doc = &self.tabs.documents[tab_index];
                let title = doc.title.clone();
                for m in matches_for(&doc.buffer, &options) {
                    let pos = char_to_pos(&doc.buffer, m.start);
                    results.push(FindAllResult {
                        source: ResultSource::Tab(tab_index),
                        location_label: title.clone(),
                        line: pos.line,
                        col: pos.col,
                        match_start: m.start,
                        match_end: m.end,
                        line_text: line_text_for(&doc.buffer, pos.line),
                    });
                }
            }
        }

        let count = results.len();
        let scope_label = if all_tabs { "all tabs" } else { "current tab" }.to_string();
        self.find_results
            .set(query.clone(), scope_label, None, results);
        self.find_replace.status = if query.trim().is_empty() {
            "Enter text to search".to_string()
        } else if count == 0 {
            "No matches".to_string()
        } else {
            format!("{count} matches")
        };
    }

    /// Jumps to the Find Results entry at `idx`, selecting the match. Open-tab
    /// results activate the tab directly; on-disk (folder-search) results open
    /// the file first, then navigate once it loads.
    pub(crate) fn navigate_to_find_result(&mut self, idx: usize) {
        let (source, start, end, line) = match self.find_results.result(idx) {
            Some(r) => (r.source.clone(), r.match_start, r.match_end, r.line),
            None => return,
        };
        match source {
            ResultSource::Tab(tab_index) => {
                self.navigate_open_tab_to_match(tab_index, start, end, line);
            }
            ResultSource::File(path) => {
                self.navigate_file_to_match(path, start, end, line);
            }
        }
        // Hand the keyboard to the editor so arrows move the cursor, not the tree.
        self.workspace_sidebar.kbd_active = false;
    }

    /// Selects the match in an already-open tab, clamping stale offsets.
    fn navigate_open_tab_to_match(
        &mut self,
        tab_index: usize,
        start: usize,
        end: usize,
        line: usize,
    ) {
        if tab_index >= self.tabs.tab_count() {
            return;
        }
        self.tabs.active = tab_index;
        let doc = &mut self.tabs.documents[tab_index];
        let len = doc.buffer.len_chars();
        if start > len {
            return; // offset no longer valid after edits; skip rather than mis-select
        }
        let mat = SearchMatch {
            start,
            end: end.min(len),
            line,
        };
        navigate_to_match(doc, &mat);
    }

    /// Navigates to a match in an on-disk file. Switches to it if already open,
    /// otherwise opens it and defers the navigation to when the read completes.
    fn navigate_file_to_match(&mut self, path: PathBuf, start: usize, end: usize, line: usize) {
        if let Some(idx) = self
            .tabs
            .documents
            .iter()
            .position(|d| d.file_path.as_deref() == Some(path.as_path()))
        {
            self.navigate_open_tab_to_match(idx, start, end, line);
            return;
        }
        self.pending_find_nav = Some(PendingFindNav {
            path: path.clone(),
            start,
            end,
            line,
        });
        self.open_file_path(&path);
    }

    /// Applies a deferred folder-search navigation once the file's tab exists.
    /// Called from the I/O response handler after a `FileRead` lands.
    pub(crate) fn apply_pending_find_nav(&mut self, path: &std::path::Path) {
        let Some(nav) = self.pending_find_nav.take_if(|n| n.path.as_path() == path) else {
            return;
        };
        if let Some(idx) = self
            .tabs
            .documents
            .iter()
            .position(|d| d.file_path.as_deref() == Some(path))
        {
            self.navigate_open_tab_to_match(idx, nav.start, nav.end, nav.line);
        }
    }

    /// Clears a deferred folder-search navigation whose file failed to open.
    pub(crate) fn clear_pending_find_nav(&mut self, path: &std::path::Path) {
        if self
            .pending_find_nav
            .as_ref()
            .is_some_and(|n| n.path.as_path() == path)
        {
            self.pending_find_nav = None;
        }
    }

    /// Handles search/replace within the active tab only.
    pub(crate) fn handle_search_current_tab(&mut self, action: FindReplaceAction) {
        match action {
            // Handled in `handle_search_action` before scope dispatch.
            FindReplaceAction::FindAll | FindReplaceAction::ChooseFolder => {}
            FindReplaceAction::Search => {
                let doc = self.tabs.active_doc_mut();
                if let Err(e) = self.find_replace.engine.find_all_versioned(
                    &doc.buffer,
                    &self.find_replace.options,
                    Some(DocRevision {
                        id: doc.id,
                        version: doc.content_version,
                    }),
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
                    Some(DocRevision {
                        id: doc.id,
                        version: doc.content_version,
                    }),
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
                    Some(DocRevision {
                        id: doc.id,
                        version: doc.content_version,
                    }),
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
            // Handled in `handle_search_action` before scope dispatch.
            FindReplaceAction::FindAll | FindReplaceAction::ChooseFolder => {}
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

#[cfg(test)]
mod tests {
    use super::*;
    use rust_pad_core::document::Document;
    use rust_pad_core::search::SearchMatch;

    /// A search in one tab must not reuse another tab's match offsets. The two
    /// tabs share a `content_version`, so the engine's match cache has to
    /// distinguish them by document identity, not version alone.
    #[test]
    fn current_tab_search_rescans_after_switching_tabs() {
        use rust_pad_core::buffer::TextBuffer;
        let mut app = super::super::tests::test_app();

        // Tab A: "text" at char 0. Tab B: "text" at char 10.
        app.tabs.documents[0].buffer = TextBuffer::from("text here");
        app.tabs.documents[0].content_version = 5;
        app.tabs.new_tab();
        app.tabs.documents[1].buffer = TextBuffer::from("zzzz zzzz text");
        app.tabs.documents[1].content_version = 5;

        app.find_replace.find_text = "text".to_string();
        app.find_replace.options.query = "text".to_string();

        app.tabs.active = 0;
        app.handle_search_action(FindReplaceAction::Search);
        assert_eq!(app.find_replace.engine.matches[0].start, 0);

        app.tabs.active = 1;
        app.handle_search_action(FindReplaceAction::Search);
        assert_eq!(
            app.find_replace.engine.matches[0].start, 10,
            "Tab B must be re-searched, not reuse Tab A's stale match offsets"
        );
    }

    #[test]
    fn test_navigate_to_match_sets_scroll_to_cursor() {
        let mut doc = Document::new();
        doc.insert_text("hello world");
        doc.scroll_to_cursor = false; // reset after insert
        let mat = SearchMatch {
            start: 6,
            end: 11,
            line: 0,
        };
        navigate_to_match(&mut doc, &mat);
        assert!(doc.scroll_to_cursor);
    }

    fn file_hit(path: PathBuf) -> FindAllResult {
        FindAllResult {
            source: ResultSource::File(path),
            location_label: "f.txt".to_string(),
            line: 0,
            col: 6,
            match_start: 6,
            match_end: 10,
            line_text: "alpha beta".to_string(),
        }
    }

    #[test]
    fn folder_result_navigation_defers_until_file_opens() {
        let mut app = super::super::tests::test_app();
        let path = std::env::temp_dir().join("rust_pad_no_such_file_xyz.txt");
        app.find_results.set(
            "beta".to_string(),
            "folder \"x\"".to_string(),
            None,
            vec![file_hit(path.clone())],
        );

        app.navigate_to_find_result(0);
        assert!(
            app.pending_find_nav.is_some(),
            "opening a not-yet-open file must defer the jump"
        );

        // A read failure for that path drops the deferred navigation.
        app.clear_pending_find_nav(&path);
        assert!(app.pending_find_nav.is_none());
    }

    #[test]
    fn folder_result_navigation_selects_already_open_file_directly() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("f.txt");
        std::fs::write(&file, "alpha beta\n").unwrap();

        let mut app = super::super::tests::test_app();
        app.tabs.open_file(&file).unwrap();
        let target = app
            .tabs
            .documents
            .iter()
            .position(|d| d.file_path.as_deref() == Some(file.as_path()))
            .unwrap();

        // Move focus to another tab so navigation has to switch back.
        app.tabs.new_tab();
        app.tabs.switch_to(app.tabs.tab_count() - 1);

        app.find_results.set(
            "beta".to_string(),
            "folder \"x\"".to_string(),
            None,
            vec![file_hit(file.clone())],
        );
        app.navigate_to_find_result(0);

        assert_eq!(app.tabs.active, target, "jumps straight to the open file");
        assert!(
            app.pending_find_nav.is_none(),
            "no deferral for an open file"
        );
    }

    #[test]
    fn set_search_folder_switches_scope_and_clears_status() {
        let mut app = super::super::tests::test_app();
        app.find_replace.status = "stale".to_string();
        app.set_search_folder(PathBuf::from("root/sub"));
        assert_eq!(app.find_replace.scope, SearchScope::Folder);
        assert_eq!(
            app.find_replace.folder_path.as_deref(),
            Some(Path::new("root/sub"))
        );
        assert!(
            app.find_replace.status.is_empty(),
            "choosing a folder clears the stale status"
        );
    }

    #[test]
    fn set_search_folder_replaces_previous_folder() {
        let mut app = super::super::tests::test_app();
        app.set_search_folder(PathBuf::from("a"));
        app.set_search_folder(PathBuf::from("b"));
        assert_eq!(
            app.find_replace.folder_path.as_deref(),
            Some(Path::new("b"))
        );
    }

    #[test]
    fn folder_search_notice_summarizes_truncation_and_skips() {
        assert!(folder_search_notice(&FolderSearchOutcome::default()).is_none());

        let truncated = FolderSearchOutcome {
            truncated: true,
            ..Default::default()
        };
        assert!(folder_search_notice(&truncated)
            .unwrap()
            .contains("truncated"));

        let skipped = FolderSearchOutcome {
            skipped_binary: 2,
            skipped_too_large: 1,
            ..Default::default()
        };
        assert!(folder_search_notice(&skipped).unwrap().contains("3 file"));
    }

    #[test]
    fn populate_folder_search_results_maps_file_hits() {
        let mut app = super::super::tests::test_app();
        app.find_replace.find_text = "hi".to_string();
        let hit = rust_pad_core::search::FolderSearchHit {
            path: PathBuf::from("root/sub/f.txt"),
            line: 3,
            col: 1,
            match_start: 10,
            match_end: 12,
            line_text: "hi there".to_string(),
        };
        let outcome = FolderSearchOutcome {
            hits: vec![hit],
            truncated: true,
            ..Default::default()
        };
        app.populate_folder_search_results(Path::new("root"), outcome);

        assert!(app.find_results.visible);
        assert_eq!(app.find_results.len(), 1);
        let r = app.find_results.result(0).unwrap();
        assert_eq!(
            r.source,
            ResultSource::File(PathBuf::from("root/sub/f.txt"))
        );
        assert!(r.location_label.contains("f.txt") && !r.location_label.contains("root"));
        assert_eq!((r.line, r.col), (3, 1));
    }
}
