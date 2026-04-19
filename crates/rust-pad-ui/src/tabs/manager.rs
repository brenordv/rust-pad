/// Tab manager for handling multiple open documents.
use std::sync::Arc;

use rust_pad_core::document::Document;
use rust_pad_core::encoding::LineEnding;
use rust_pad_core::history::{HistoryConfig, PersistenceLayer};

use super::split::{PaneId, PaneTabSplit};

/// Default line ending policy for new documents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DefaultLineEnding {
    /// Use the OS default (`CRLF` on Windows, `LF` elsewhere).
    System,
    /// Always use `LF` (Unix/macOS).
    Unix,
    /// Always use `CRLF` (Windows).
    Windows,
}

impl DefaultLineEnding {
    /// Resolves this policy to a concrete `LineEnding`.
    pub fn resolve(self) -> LineEnding {
        match self {
            Self::System => LineEnding::default(),
            Self::Unix => LineEnding::Lf,
            Self::Windows => LineEnding::CrLf,
        }
    }

    /// Parses from a config string. Unrecognised values fall back to `System`.
    pub fn from_config(s: &str) -> Self {
        match s {
            "lf" => Self::Unix,
            "crlf" => Self::Windows,
            _ => Self::System,
        }
    }

    /// Returns the config string representation.
    pub fn as_config_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Unix => "lf",
            Self::Windows => "crlf",
        }
    }
}

/// Manages open document tabs.
#[derive(Debug)]
pub struct TabManager {
    /// All open documents.
    pub documents: Vec<Document>,
    /// Document index of the focused pane's active tab.
    ///
    /// In single-pane mode, this is the only "active" concept. In split-pane
    /// mode, it mirrors `panes.as_ref().unwrap().active_for(focused)` and is
    /// kept in sync by every mutator that touches per-pane state.
    pub active: usize,
    /// Per-pane tab assignment when split view is active. `None` in
    /// single-pane mode, which is the default.
    pub panes: Option<PaneTabSplit>,
    /// Shared persistence layer for undo history (None = in-memory only).
    persistence: Option<Arc<PersistenceLayer>>,
    /// History configuration.
    config: HistoryConfig,
    /// Default file extension for new untitled tabs (e.g. "txt", "md"). Empty = none.
    pub default_extension: String,
    /// Default line ending for new documents.
    pub default_line_ending: DefaultLineEnding,
}

impl Default for TabManager {
    fn default() -> Self {
        Self::new()
    }
}

impl TabManager {
    /// Creates a new tab manager with one empty document (in-memory history).
    pub fn new() -> Self {
        Self {
            documents: vec![Document::new()],
            active: 0,
            panes: None,
            persistence: None,
            config: HistoryConfig::default(),
            default_extension: String::new(),
            default_line_ending: DefaultLineEnding::System,
        }
    }

    /// Creates a new tab manager with persistent undo history.
    pub fn with_persistence(persistence: Arc<PersistenceLayer>, config: HistoryConfig) -> Self {
        let doc = Document::with_persistence(Arc::clone(&persistence), &config);
        Self {
            documents: vec![doc],
            active: 0,
            panes: None,
            persistence: Some(persistence),
            config,
            default_extension: String::new(),
            default_line_ending: DefaultLineEnding::System,
        }
    }

    /// Returns the active document.
    pub fn active_doc(&self) -> &Document {
        &self.documents[self.active]
    }

    /// Returns the active document mutably.
    pub fn active_doc_mut(&mut self) -> &mut Document {
        &mut self.documents[self.active]
    }

    /// Adds a new empty document tab and switches to it.
    ///
    /// The tab receives a numbered "Untitled" title that avoids
    /// collisions with existing tabs ("Untitled", "Untitled 2", …).
    /// When `default_extension` is set, the title includes the extension
    /// (e.g. "Untitled.txt", "Untitled 2.md").
    pub fn new_tab(&mut self) {
        let title = self.next_untitled_title();
        let mut doc = self.create_document();
        doc.title = title;
        self.push_doc_and_activate(doc);
    }

    /// Appends `doc` to `documents`, assigns it to the focused pane in
    /// split mode, and makes it the active tab.
    ///
    /// Every "open new tab" path goes through here so that newly created
    /// documents land in whichever pane the user was looking at, and so
    /// that no caller can forget the per-pane assignment step.
    fn push_doc_and_activate(&mut self, doc: Document) {
        self.documents.push(doc);
        let new_idx = self.documents.len() - 1;
        self.assign_new_doc_to_focused_pane(new_idx);
        self.active = new_idx;
    }

    /// If a document with `path` is already open, switches to it and
    /// returns `true`. Returns `false` otherwise.
    fn switch_to_path_if_open(&mut self, path: &std::path::Path) -> bool {
        for (idx, doc) in self.documents.iter().enumerate() {
            if doc.file_path.as_deref() == Some(path) {
                self.switch_to(idx);
                return true;
            }
        }
        false
    }

    /// Returns the next available "Untitled" title.
    ///
    /// Finds the highest existing "Untitled" number and returns the next one.
    /// Strips any file extension from existing titles before matching, so
    /// "Untitled.txt" and "Untitled" both count as number 1.
    ///
    /// Numbers always increase: "Untitled", "Untitled 2", "Untitled 3", etc.
    /// Closing an earlier tab does not cause its number to be reused.
    ///
    /// When `default_extension` is non-empty, appends it to the title
    /// (e.g. "Untitled.txt", "Untitled 2.md").
    pub fn next_untitled_title(&self) -> String {
        let mut max_n = 0usize;
        for doc in &self.documents {
            let n = Self::parse_untitled_number(&doc.title);
            max_n = max_n.max(n);
        }
        let next = max_n + 1;
        let name = if next == 1 {
            "Untitled".to_string()
        } else {
            format!("Untitled {next}")
        };
        if self.default_extension.is_empty() {
            name
        } else {
            format!("{name}.{}", self.default_extension)
        }
    }

    /// Extracts the untitled number from a tab title, or 0 if not an untitled tab.
    ///
    /// Handles titles with or without file extensions:
    /// - "Untitled" or "Untitled.txt" → 1
    /// - "Untitled 3" or "Untitled 3.md" → 3
    /// - "myfile.rs" → 0
    fn parse_untitled_number(title: &str) -> usize {
        // Strip any file extension
        let base = match title.rfind('.') {
            Some(pos) if pos > 0 => &title[..pos],
            _ => title,
        };
        if base == "Untitled" {
            1
        } else if let Some(suffix) = base.strip_prefix("Untitled ") {
            suffix.parse::<usize>().unwrap_or(0)
        } else {
            0
        }
    }

    /// Opens a document from file and adds it as a new tab.
    pub fn open_file(&mut self, path: &std::path::Path) -> anyhow::Result<()> {
        if self.switch_to_path_if_open(path) {
            return Ok(());
        }
        let doc = match &self.persistence {
            Some(pl) => Document::open_with_persistence(path, Arc::clone(pl), &self.config)?,
            None => Document::open(path)?,
        };
        self.push_doc_and_activate(doc);
        Ok(())
    }

    /// Creates a document from pre-read bytes and adds it as a new tab.
    ///
    /// Used by the async I/O path when file bytes arrive from a background
    /// thread. Handles duplicate detection the same way as `open_file`.
    pub fn open_from_bytes(&mut self, path: &std::path::Path, bytes: &[u8]) -> anyhow::Result<()> {
        if self.switch_to_path_if_open(path) {
            return Ok(());
        }
        let doc = match &self.persistence {
            Some(pl) => Document::from_bytes(bytes, path, Some((Arc::clone(pl), &self.config)))?,
            None => Document::from_bytes(bytes, path, None)?,
        };
        self.push_doc_and_activate(doc);
        Ok(())
    }

    /// Closes the active tab. Returns true if the tab was closed.
    /// The caller should check for unsaved changes before calling this.
    pub fn close_active(&mut self) -> bool {
        let idx = self.active;
        self.close_tab(idx)
    }

    /// Closes a tab by index. Returns true if closed.
    ///
    /// In split-view mode, closing the last tab in a pane collapses the
    /// split; the surviving pane becomes the only pane.
    pub fn close_tab(&mut self, idx: usize) -> bool {
        if idx >= self.documents.len() {
            return false;
        }

        if self.documents.len() <= 1 {
            self.delete_tab_history(0);
            self.documents[0] = self.create_document();
            self.active = 0;
            // Reset any split state — there is only one document now.
            self.panes = None;
            return true;
        }

        self.delete_tab_history(idx);
        self.documents.remove(idx);

        // Update split-view state: drop the closed index from each pane's
        // tab order and rewrite indices that shifted left, then collapse
        // the split if either pane became empty.
        if let Some(panes) = self.panes.as_mut() {
            Self::renumber_pane_after_remove(&mut panes.left_order, &mut panes.left_active, idx);
            Self::renumber_pane_after_remove(&mut panes.right_order, &mut panes.right_active, idx);
            if panes.left_order.is_empty() || panes.right_order.is_empty() {
                self.panes = None;
            }
        }

        if self.active >= self.documents.len() {
            self.active = self.documents.len() - 1;
        } else if self.active > idx {
            self.active -= 1;
        }

        // Re-sync `active` with the focused pane in split mode.
        if let Some(panes) = self.panes.as_ref() {
            self.active = panes.active_for(panes.focused);
        }
        true
    }

    /// Removes `removed` from `order` (if present) and decrements every
    /// remaining index that was greater than `removed`.
    fn remove_and_renumber(order: &mut Vec<usize>, removed: usize) {
        order.retain(|&i| i != removed);
        for i in order.iter_mut() {
            if *i > removed {
                *i -= 1;
            }
        }
    }

    /// Drops `removed` from a pane's `order` and rewrites the pane's
    /// `active` doc index so it continues to point at a valid document.
    /// If the active doc was the one being removed, falls back to the
    /// pane's first survivor (or `0` for an empty pane — caller must
    /// then collapse the split).
    fn renumber_pane_after_remove(order: &mut Vec<usize>, active: &mut usize, removed: usize) {
        Self::remove_and_renumber(order, removed);
        if *active == removed {
            *active = order.first().copied().unwrap_or(0);
        } else if *active > removed {
            *active -= 1;
        }
    }

    /// Switches to a specific tab.
    ///
    /// In split-view mode, switching to a tab also focuses the pane that
    /// owns it (so the editor reflects the user's selection unambiguously).
    pub fn switch_to(&mut self, idx: usize) {
        if idx >= self.documents.len() {
            return;
        }
        if let Some(panes) = self.panes.as_mut() {
            if let Some(pane) = panes.pane_of(idx) {
                panes.set_active(pane, idx);
                panes.focused = pane;
            }
        }
        self.active = idx;
    }

    /// Returns the number of open tabs.
    pub fn tab_count(&self) -> usize {
        self.documents.len()
    }

    /// Returns the number of pinned tabs.
    ///
    /// Pinned tabs are always kept at the start of `documents`, so this is
    /// equivalent to the index of the first unpinned tab (or `tab_count()`
    /// if every tab is pinned).
    pub fn pinned_count(&self) -> usize {
        self.documents
            .iter()
            .position(|d| !d.pinned)
            .unwrap_or(self.documents.len())
    }

    /// Pins the tab at `idx`. The tab is moved to the rightmost position
    /// among pinned tabs and `self.active` is updated to track whichever
    /// document was active before the call.
    ///
    /// No-op if `idx` is out of range or the tab is already pinned.
    pub fn pin_tab(&mut self, idx: usize) {
        if idx >= self.documents.len() || self.documents[idx].pinned {
            return;
        }
        self.documents[idx].pinned = true;
        // The total number of pinned tabs (including the one we just
        // flipped) tells us how big the pinned section will be after the
        // move. The new tab goes to the last slot of that section.
        let total_pinned = self.documents.iter().filter(|d| d.pinned).count();
        let target = total_pinned - 1;
        self.move_tab(idx, target);
    }

    /// Unpins the tab at `idx`. The tab is moved to the leftmost position
    /// among unpinned tabs and `self.active` is updated to track whichever
    /// document was active before the call.
    ///
    /// No-op if `idx` is out of range or the tab is not pinned.
    pub fn unpin_tab(&mut self, idx: usize) {
        if idx >= self.documents.len() || !self.documents[idx].pinned {
            return;
        }
        self.documents[idx].pinned = false;
        // The remaining pinned count tells us where the unpinned section
        // starts after the move. The newly-unpinned tab goes there.
        let total_pinned = self.documents.iter().filter(|d| d.pinned).count();
        let target = total_pinned;
        self.move_tab(idx, target);
    }

    /// Moves a tab from `from` to `to` while keeping `self.active` pointing
    /// at the same `Document` it pointed to before the move.
    ///
    /// No-op when `from == to` or either index is out of range. Callers are
    /// responsible for any domain-level constraints (e.g. keeping pinned
    /// tabs clamped to the pinned section during drag-and-drop).
    pub fn move_tab(&mut self, from: usize, to: usize) {
        if from == to || from >= self.documents.len() || to >= self.documents.len() {
            return;
        }
        let doc = self.documents.remove(from);
        self.documents.insert(to, doc);

        // Track which Document the active index pointed to.
        Self::remap_after_move(&mut self.active, from, to);

        // Re-map every per-pane index through the same shift so that the
        // pane tab orders continue to point at the same Documents.
        if let Some(panes) = self.panes.as_mut() {
            for i in panes.left_order.iter_mut() {
                Self::remap_after_move(i, from, to);
            }
            for i in panes.right_order.iter_mut() {
                Self::remap_after_move(i, from, to);
            }
            Self::remap_after_move(&mut panes.left_active, from, to);
            Self::remap_after_move(&mut panes.right_active, from, to);
        }
    }

    /// Updates a stored document index after a `documents.remove(from); insert(to)`
    /// operation, so that it continues to point at the same `Document`.
    fn remap_after_move(idx: &mut usize, from: usize, to: usize) {
        if *idx == from {
            *idx = to;
        } else if from < *idx && to >= *idx {
            *idx -= 1;
        } else if from > *idx && to <= *idx {
            *idx += 1;
        }
    }

    /// Flushes undo history for all open documents to disk.
    pub fn flush_all_history(&mut self) {
        for doc in &mut self.documents {
            if let Err(e) = doc.flush_history() {
                tracing::warn!("Failed to flush history for '{}': {e}", doc.title);
            }
        }
    }

    /// Creates a new empty document with the appropriate persistence setting.
    fn create_document(&self) -> Document {
        let mut doc = match &self.persistence {
            Some(pl) => Document::with_persistence(Arc::clone(pl), &self.config),
            None => Document::new(),
        };
        doc.line_ending = self.default_line_ending.resolve();
        doc
    }

    /// Deletes persisted history for a tab that is being closed.
    fn delete_tab_history(&mut self, idx: usize) {
        if idx < self.documents.len() {
            if let Err(e) = self.documents[idx].delete_history() {
                tracing::warn!(
                    "Failed to delete history for '{}': {e}",
                    self.documents[idx].title
                );
            }
        }
    }

    // ── Split-view API ───────────────────────────────────────────────

    /// Returns true if the split view is active.
    pub fn is_split(&self) -> bool {
        self.panes.is_some()
    }

    /// Returns which pane currently has focus. In single-pane mode this
    /// is always `PaneId::Left`.
    pub fn focused_pane(&self) -> PaneId {
        self.panes.as_ref().map_or(PaneId::Left, |p| p.focused)
    }

    /// Returns the document indices owned by the given pane, in display order.
    ///
    /// In single-pane mode the Left pane owns every document and the Right
    /// pane owns none.
    pub fn pane_tab_order(&self, pane: PaneId) -> Vec<usize> {
        match (&self.panes, pane) {
            (Some(p), _) => p.order_for(pane).to_vec(),
            (None, PaneId::Left) => (0..self.documents.len()).collect(),
            (None, PaneId::Right) => Vec::new(),
        }
    }

    /// Returns the active document index for the given pane.
    pub fn pane_active_doc(&self, pane: PaneId) -> usize {
        match &self.panes {
            Some(p) => p.active_for(pane),
            None => self.active,
        }
    }

    /// Returns the active document for the given pane (read-only).
    pub fn pane_active_doc_ref(&self, pane: PaneId) -> &Document {
        &self.documents[self.pane_active_doc(pane)]
    }

    /// Returns the active document for the given pane (mutable).
    pub fn pane_active_doc_mut(&mut self, pane: PaneId) -> &mut Document {
        let idx = self.pane_active_doc(pane);
        &mut self.documents[idx]
    }

    /// Sets which pane is focused. Updates `active` to that pane's
    /// active doc so that the rest of the app keeps operating on the
    /// "currently visible" document. No-op when not split.
    pub fn focus_pane(&mut self, pane: PaneId) {
        if let Some(panes) = self.panes.as_mut() {
            panes.focused = pane;
            self.active = panes.active_for(pane);
        }
    }

    /// Switches the given pane to a specific document. Both the pane's
    /// active tab and the global `active` (when the pane is focused) are
    /// updated.
    pub fn switch_pane_to(&mut self, pane: PaneId, doc_idx: usize) {
        if doc_idx >= self.documents.len() {
            return;
        }
        if let Some(panes) = self.panes.as_mut() {
            // Only honour the switch if the pane actually owns the doc.
            let owns = panes.order_for(pane).contains(&doc_idx);
            if !owns {
                return;
            }
            panes.set_active(pane, doc_idx);
            if panes.focused == pane {
                self.active = doc_idx;
            }
        } else if pane == PaneId::Left {
            self.active = doc_idx;
        }
    }

    /// Enables split view by partitioning the existing tabs across two panes.
    ///
    /// The previously-active document is moved to the right pane (becoming
    /// the new focused pane). All other documents go to the left pane. If
    /// only one tab is open, a fresh untitled tab is created and placed in
    /// the right pane so the user immediately sees two distinct documents.
    ///
    /// No-op when split view is already active.
    pub fn enable_split(&mut self) {
        if self.panes.is_some() {
            return;
        }

        if self.documents.is_empty() {
            return;
        }

        // Single-tab case: create a fresh tab so the right pane has content.
        if self.documents.len() == 1 {
            // Create the new doc directly to avoid `new_tab` trying to
            // assign it to a pane that does not yet exist.
            let title = self.next_untitled_title();
            let mut doc = self.create_document();
            doc.title = title;
            self.documents.push(doc);
            self.panes = Some(PaneTabSplit {
                left_order: vec![0],
                right_order: vec![1],
                left_active: 0,
                right_active: 1,
                focused: PaneId::Right,
            });
            self.active = 1;
            return;
        }

        let active = self.active;
        let n = self.documents.len();
        let left_order: Vec<usize> = (0..n).filter(|&i| i != active).collect();
        let right_order = vec![active];
        let left_active = left_order.first().copied().unwrap_or(active);

        self.panes = Some(PaneTabSplit {
            left_order,
            right_order,
            left_active,
            right_active: active,
            focused: PaneId::Right,
        });
        self.active = active;
    }

    /// Disables split view, returning to single-pane mode.
    ///
    /// All tabs from the right pane are kept; pane membership is simply
    /// dropped, leaving the flat `documents` order intact.
    pub fn disable_split(&mut self) {
        self.panes = None;
    }

    /// Moves a document from its current pane to the target pane.
    ///
    /// If the source pane is left empty by this move, the split is
    /// collapsed automatically. No-op when not split or when the document
    /// already lives in the target pane.
    pub fn move_tab_to_pane(&mut self, doc_idx: usize, target: PaneId) {
        let Some(panes) = self.panes.as_mut() else {
            return;
        };
        if doc_idx >= self.documents.len() {
            return;
        }
        let Some(source) = panes.pane_of(doc_idx) else {
            return;
        };
        if source == target {
            return;
        }

        // Remove from source pane.
        panes.order_for_mut(source).retain(|&i| i != doc_idx);
        // Update source pane's active doc if it pointed at the moved tab.
        // Picks any survivor; the empty-source case is handled below.
        let new_src_active = panes.order_for(source).first().copied().unwrap_or(doc_idx);
        let src_active_ref = panes.active_mut(source);
        if *src_active_ref == doc_idx {
            *src_active_ref = new_src_active;
        }

        // Append to target pane and make it active there.
        let tgt_order = panes.order_for_mut(target);
        if !tgt_order.contains(&doc_idx) {
            tgt_order.push(doc_idx);
        }
        panes.set_active(target, doc_idx);
        panes.focused = target;
        self.active = doc_idx;

        // If the source pane is now empty, collapse the split.
        if panes.order_for(source).is_empty() {
            self.panes = None;
        }
    }

    /// Internal helper: appends a freshly added document index to the
    /// focused pane's tab order (no-op when not split). Called by every
    /// "open new tab" path so that newly created documents land in the
    /// pane the user was looking at.
    fn assign_new_doc_to_focused_pane(&mut self, doc_idx: usize) {
        if let Some(panes) = self.panes.as_mut() {
            let pane = panes.focused;
            let order = panes.order_for_mut(pane);
            if !order.contains(&doc_idx) {
                order.push(doc_idx);
            }
            panes.set_active(pane, doc_idx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_tab_manager() {
        let tm = TabManager::new();
        assert_eq!(tm.tab_count(), 1);
        assert_eq!(tm.active, 0);
    }

    #[test]
    fn test_new_tab() {
        let mut tm = TabManager::new();
        tm.new_tab();
        assert_eq!(tm.tab_count(), 2);
        assert_eq!(tm.active, 1);
    }

    #[test]
    fn test_close_active() {
        let mut tm = TabManager::new();
        tm.new_tab();
        tm.new_tab();
        assert_eq!(tm.tab_count(), 3);
        tm.switch_to(1);
        tm.close_active();
        assert_eq!(tm.tab_count(), 2);
    }

    #[test]
    fn test_close_last_tab() {
        let mut tm = TabManager::new();
        tm.active_doc_mut().insert_text("hello");
        tm.close_active();
        assert_eq!(tm.tab_count(), 1);
        assert!(tm.active_doc().buffer.is_empty());
    }

    #[test]
    fn test_switch_to() {
        let mut tm = TabManager::new();
        tm.new_tab();
        tm.switch_to(0);
        assert_eq!(tm.active, 0);
    }

    // ── tab_count ────────────────────────────────────────────────────

    #[test]
    fn test_tab_count() {
        let mut tm = TabManager::new();
        assert_eq!(tm.tab_count(), 1);
        tm.new_tab();
        assert_eq!(tm.tab_count(), 2);
        tm.new_tab();
        assert_eq!(tm.tab_count(), 3);
    }

    // ── close_tab by index ───────────────────────────────────────────

    #[test]
    fn test_close_tab_by_index() {
        let mut tm = TabManager::new();
        tm.new_tab();
        tm.new_tab();
        assert_eq!(tm.tab_count(), 3);
        tm.close_tab(1);
        assert_eq!(tm.tab_count(), 2);
    }

    #[test]
    fn test_close_tab_out_of_bounds() {
        let mut tm = TabManager::new();
        tm.new_tab();
        let result = tm.close_tab(10);
        assert!(!result);
        assert_eq!(tm.tab_count(), 2);
    }

    // ── Active index adjustment on close ─────────────────────────────

    #[test]
    fn test_close_tab_adjusts_active_when_before() {
        let mut tm = TabManager::new();
        tm.new_tab();
        tm.new_tab();
        tm.switch_to(2); // active = 2
        tm.close_tab(0); // close tab before active
                         // Active should shift from 2 to 1
        assert_eq!(tm.active, 1);
        assert_eq!(tm.tab_count(), 2);
    }

    #[test]
    fn test_close_tab_adjusts_active_when_at_end() {
        let mut tm = TabManager::new();
        tm.new_tab();
        tm.new_tab();
        tm.switch_to(2); // active = 2
        tm.close_tab(2); // close the active tab
                         // Active was at end, should clamp to new last index
        assert_eq!(tm.active, 1);
        assert_eq!(tm.tab_count(), 2);
    }

    #[test]
    fn test_close_tab_after_active_unchanged() {
        let mut tm = TabManager::new();
        tm.new_tab();
        tm.new_tab();
        tm.switch_to(0); // active = 0
        tm.close_tab(2); // close tab after active
        assert_eq!(tm.active, 0); // unchanged
        assert_eq!(tm.tab_count(), 2);
    }

    // ── close_active with single tab resets ──────────────────────────

    #[test]
    fn test_close_active_single_tab_resets_to_empty() {
        let mut tm = TabManager::new();
        tm.active_doc_mut().insert_text("some content");
        assert!(!tm.active_doc().buffer.is_empty());
        tm.close_active();
        assert_eq!(tm.tab_count(), 1);
        assert!(tm.active_doc().buffer.is_empty());
        assert_eq!(tm.active, 0);
    }

    // ── switch_to out of bounds ──────────────────────────────────────

    #[test]
    fn test_switch_to_out_of_bounds() {
        let mut tm = TabManager::new();
        tm.switch_to(100); // should be ignored
        assert_eq!(tm.active, 0);
    }

    // ── open_file duplicate detection ────────────────────────────────

    #[test]
    fn test_open_file_duplicate_switches_tab() {
        let dir = std::env::temp_dir().join("rust_pad_test_dedup");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("test.txt");
        std::fs::write(&path, "hello").unwrap();

        let mut tm = TabManager::new();
        tm.open_file(&path).unwrap();
        assert_eq!(tm.tab_count(), 2);
        assert_eq!(tm.active, 1);

        // Opening same file again should NOT add a new tab
        tm.switch_to(0);
        tm.open_file(&path).unwrap();
        assert_eq!(tm.tab_count(), 2);
        assert_eq!(tm.active, 1); // switched to existing tab

        std::fs::remove_dir_all(&dir).ok();
    }

    // ── active_doc and active_doc_mut ────────────────────────────────

    #[test]
    fn test_active_doc_returns_correct_tab() {
        let mut tm = TabManager::new();
        tm.active_doc_mut().insert_text("tab0");
        tm.new_tab();
        tm.active_doc_mut().insert_text("tab1");

        tm.switch_to(0);
        assert_eq!(tm.active_doc().buffer.to_string(), "tab0");

        tm.switch_to(1);
        assert_eq!(tm.active_doc().buffer.to_string(), "tab1");
    }

    // ── close_tab with single tab resets ─────────────────────────────

    #[test]
    fn test_close_tab_single_tab_resets() {
        let mut tm = TabManager::new();
        tm.active_doc_mut().insert_text("content");
        let result = tm.close_tab(0);
        assert!(result);
        assert_eq!(tm.tab_count(), 1);
        assert!(tm.active_doc().buffer.is_empty());
    }

    // ── Default trait ────────────────────────────────────────────────

    #[test]
    fn test_default() {
        let tm = TabManager::default();
        assert_eq!(tm.tab_count(), 1);
        assert_eq!(tm.active, 0);
        assert!(tm.active_doc().buffer.is_empty());
    }

    // ── new_tab always appends and activates ─────────────────────────

    #[test]
    fn test_new_tab_activates_last() {
        let mut tm = TabManager::new();
        tm.new_tab();
        tm.new_tab();
        tm.new_tab();
        assert_eq!(tm.tab_count(), 4);
        assert_eq!(tm.active, 3);
    }

    // ── close_active in multi-tab scenario ───────────────────────────

    #[test]
    fn test_close_active_middle_tab() {
        let mut tm = TabManager::new();
        tm.new_tab();
        tm.new_tab();
        tm.switch_to(1);
        tm.close_active();
        assert_eq!(tm.tab_count(), 2);
        // After closing tab 1, active should be clamped
        assert!(tm.active < tm.tab_count());
    }

    // ── untitled tab numbering ───────────────────────────────────────

    #[test]
    fn test_new_tab_numbered_titles() {
        let mut tm = TabManager::new();
        // Initial tab is "Untitled"
        assert_eq!(tm.documents[0].title, "Untitled");

        // Second tab: "Untitled 2"
        tm.new_tab();
        assert_eq!(tm.documents[1].title, "Untitled 2");

        // Third tab: "Untitled 3"
        tm.new_tab();
        assert_eq!(tm.documents[2].title, "Untitled 3");
    }

    #[test]
    fn test_new_tab_continues_from_highest() {
        let mut tm = TabManager::new();
        tm.new_tab(); // "Untitled 2"
        tm.new_tab(); // "Untitled 3"

        // Close "Untitled 2" (index 1)
        tm.close_tab(1);
        assert_eq!(tm.tab_count(), 2);

        // Next tab should be "Untitled 4", not reuse "Untitled 2"
        tm.new_tab();
        assert_eq!(tm.documents.last().unwrap().title, "Untitled 4");
    }

    #[test]
    fn test_new_tab_after_closing_first_untitled() {
        let mut tm = TabManager::new();
        tm.new_tab(); // "Untitled 2"

        // Close "Untitled" (index 0)
        tm.close_tab(0);
        assert_eq!(tm.documents[0].title, "Untitled 2");

        // Next tab should be "Untitled 3", continuing from highest
        tm.new_tab();
        assert_eq!(tm.documents.last().unwrap().title, "Untitled 3");
    }

    #[test]
    fn test_close_last_tab_resets_to_untitled() {
        let mut tm = TabManager::new();
        tm.new_tab(); // "Untitled 2"
        tm.switch_to(0);
        tm.close_active(); // Closes "Untitled", only "Untitled 2" remains
        tm.close_active(); // Resets single tab to new "Untitled"
        assert_eq!(tm.tab_count(), 1);
        assert_eq!(tm.documents[0].title, "Untitled");
    }

    #[test]
    fn test_next_untitled_title_skips_non_matching() {
        let mut tm = TabManager::new();
        // Rename the initial tab to something else
        tm.documents[0].title = "my_file.txt".to_string();

        // next_untitled_title should return "Untitled" (slot 1 is free)
        assert_eq!(tm.next_untitled_title(), "Untitled");

        tm.new_tab();
        assert_eq!(tm.documents[1].title, "Untitled");
    }

    // ── default extension ──────────────────────────────────────────

    #[test]
    fn test_new_tab_with_extension() {
        let mut tm = TabManager::new();
        tm.default_extension = "txt".to_string();

        tm.new_tab();
        assert_eq!(tm.documents[1].title, "Untitled 2.txt");

        tm.new_tab();
        assert_eq!(tm.documents[2].title, "Untitled 3.txt");
    }

    #[test]
    fn test_numbering_continues_across_extension_change() {
        let mut tm = TabManager::new();
        tm.default_extension = "txt".to_string();

        tm.new_tab(); // "Untitled 2.txt"
        assert_eq!(tm.documents[1].title, "Untitled 2.txt");

        // Change extension mid-session
        tm.default_extension = "md".to_string();
        tm.new_tab(); // Should be "Untitled 3.md", not "Untitled 2.md"
        assert_eq!(tm.documents[2].title, "Untitled 3.md");
    }

    #[test]
    fn test_numbering_handles_mixed_extensions() {
        let mut tm = TabManager::new();
        // Existing tab has no extension ("Untitled")
        tm.default_extension = "rs".to_string();

        tm.new_tab(); // "Untitled 2.rs"
        assert_eq!(tm.documents[1].title, "Untitled 2.rs");

        // Remove extension
        tm.default_extension.clear();
        tm.new_tab(); // "Untitled 3" (no extension)
        assert_eq!(tm.documents[2].title, "Untitled 3");
    }

    #[test]
    fn test_parse_untitled_number() {
        assert_eq!(TabManager::parse_untitled_number("Untitled"), 1);
        assert_eq!(TabManager::parse_untitled_number("Untitled 5"), 5);
        assert_eq!(TabManager::parse_untitled_number("Untitled.txt"), 1);
        assert_eq!(TabManager::parse_untitled_number("Untitled 3.md"), 3);
        assert_eq!(TabManager::parse_untitled_number("myfile.rs"), 0);
        assert_eq!(TabManager::parse_untitled_number("Untitled abc"), 0);
    }

    #[test]
    fn test_default_extension_empty_by_default() {
        let tm = TabManager::new();
        assert!(tm.default_extension.is_empty());
    }

    // ── DefaultLineEnding ───────────────────────────────────────────

    #[test]
    fn test_default_line_ending_system_by_default() {
        let tm = TabManager::new();
        assert_eq!(tm.default_line_ending, DefaultLineEnding::System);
    }

    #[test]
    fn test_default_line_ending_from_config() {
        assert_eq!(
            DefaultLineEnding::from_config("lf"),
            DefaultLineEnding::Unix
        );
        assert_eq!(
            DefaultLineEnding::from_config("crlf"),
            DefaultLineEnding::Windows
        );
        assert_eq!(
            DefaultLineEnding::from_config("system"),
            DefaultLineEnding::System
        );
        assert_eq!(
            DefaultLineEnding::from_config("unknown"),
            DefaultLineEnding::System
        );
    }

    #[test]
    fn test_default_line_ending_as_config_str() {
        assert_eq!(DefaultLineEnding::System.as_config_str(), "system");
        assert_eq!(DefaultLineEnding::Unix.as_config_str(), "lf");
        assert_eq!(DefaultLineEnding::Windows.as_config_str(), "crlf");
    }

    #[test]
    fn test_default_line_ending_resolve() {
        assert_eq!(DefaultLineEnding::Unix.resolve(), LineEnding::Lf);
        assert_eq!(DefaultLineEnding::Windows.resolve(), LineEnding::CrLf);
        // System resolves to platform default
        assert_eq!(DefaultLineEnding::System.resolve(), LineEnding::default());
    }

    #[test]
    fn test_new_tab_uses_default_line_ending() {
        let mut tm = TabManager::new();
        tm.default_line_ending = DefaultLineEnding::Unix;
        tm.new_tab();
        assert_eq!(tm.documents.last().unwrap().line_ending, LineEnding::Lf);
    }

    // ── open_from_bytes ─────────────────────────────────────────────

    #[test]
    fn test_open_from_bytes_creates_tab() {
        let dir = std::env::temp_dir().join("rust_pad_test_from_bytes");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("test.txt");
        let content = b"hello from bytes";

        let mut tm = TabManager::new();
        tm.open_from_bytes(&path, content).unwrap();
        assert_eq!(tm.tab_count(), 2);
        assert_eq!(tm.active, 1);
        assert_eq!(tm.active_doc().buffer.to_string(), "hello from bytes");
        assert_eq!(tm.active_doc().title, "test.txt");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_open_from_bytes_duplicate_switches() {
        let dir = std::env::temp_dir().join("rust_pad_test_from_bytes_dup");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("test.txt");
        std::fs::write(&path, "original").unwrap();

        let mut tm = TabManager::new();
        // First open via normal path
        tm.open_file(&path).unwrap();
        assert_eq!(tm.tab_count(), 2);

        // Switch away
        tm.switch_to(0);
        assert_eq!(tm.active, 0);

        // Open same file via from_bytes — should switch, not add
        tm.open_from_bytes(&path, b"original").unwrap();
        assert_eq!(tm.tab_count(), 2); // no new tab
        assert_eq!(tm.active, 1); // switched to existing

        std::fs::remove_dir_all(&dir).ok();
    }

    // ── Pin / unpin ─────────────────────────────────────────────────

    /// Builds a tab manager with `n` named tabs (no real files).
    fn make_n_tabs(n: usize) -> TabManager {
        let mut tm = TabManager::new();
        tm.documents[0].title = "tab0".to_string();
        for i in 1..n {
            tm.new_tab();
            tm.documents[i].title = format!("tab{i}");
        }
        tm
    }

    #[test]
    fn test_pinned_count_default_zero() {
        let tm = make_n_tabs(3);
        assert_eq!(tm.pinned_count(), 0);
    }

    #[test]
    fn test_pin_tab_moves_to_pinned_section_end() {
        let mut tm = make_n_tabs(4);
        // Pin tab 2 ("tab2") — should move to index 0 (no pinned yet).
        tm.pin_tab(2);
        assert_eq!(tm.pinned_count(), 1);
        assert_eq!(tm.documents[0].title, "tab2");
        assert!(tm.documents[0].pinned);
        // Pin tab 3 ("tab3", now at index 3) — should move to index 1.
        tm.pin_tab(3);
        assert_eq!(tm.pinned_count(), 2);
        assert_eq!(tm.documents[0].title, "tab2");
        assert_eq!(tm.documents[1].title, "tab3");
        assert!(tm.documents[1].pinned);
    }

    #[test]
    fn test_unpin_tab_moves_to_unpinned_section_start() {
        let mut tm = make_n_tabs(4);
        tm.pin_tab(0); // tab0 → index 0 (pinned)
        tm.pin_tab(2); // tab2 → index 1 (pinned). Order: [tab0*, tab2*, tab1, tab3]
        assert_eq!(tm.pinned_count(), 2);

        // Unpin tab0 (idx 0) — should move to leftmost unpinned position (idx 1).
        tm.unpin_tab(0);
        assert_eq!(tm.pinned_count(), 1);
        assert_eq!(tm.documents[0].title, "tab2");
        assert_eq!(tm.documents[1].title, "tab0");
        assert!(!tm.documents[1].pinned);
    }

    #[test]
    fn test_pin_tab_idempotent() {
        let mut tm = make_n_tabs(3);
        tm.pin_tab(1);
        let snapshot: Vec<_> = tm.documents.iter().map(|d| d.title.clone()).collect();
        tm.pin_tab(0); // already pinned (was tab1, now at idx 0)
        let after: Vec<_> = tm.documents.iter().map(|d| d.title.clone()).collect();
        assert_eq!(snapshot, after);
        assert_eq!(tm.pinned_count(), 1);
    }

    #[test]
    fn test_unpin_tab_idempotent() {
        let mut tm = make_n_tabs(3);
        // Unpin a tab that isn't pinned — no-op.
        tm.unpin_tab(1);
        assert_eq!(tm.pinned_count(), 0);
        assert!(!tm.documents[1].pinned);
    }

    #[test]
    fn test_pin_tab_out_of_range_noop() {
        let mut tm = make_n_tabs(2);
        tm.pin_tab(99);
        assert_eq!(tm.pinned_count(), 0);
    }

    #[test]
    fn test_pinning_active_tab_keeps_same_document_active() {
        let mut tm = make_n_tabs(4);
        tm.switch_to(2); // active = "tab2"
        tm.pin_tab(2);
        // After pin, "tab2" should be at index 0 and still active.
        assert_eq!(tm.documents[tm.active].title, "tab2");
        assert_eq!(tm.active, 0);
    }

    #[test]
    fn test_pinning_non_active_tab_after_active_keeps_same_document() {
        let mut tm = make_n_tabs(4);
        tm.switch_to(1); // active = "tab1"
        tm.pin_tab(3); // pin a tab to the right of active
                       // tab1 was not moved; tab3 jumped to idx 0; so active should now be 2.
        assert_eq!(tm.documents[tm.active].title, "tab1");
    }

    #[test]
    fn test_pinning_non_active_tab_before_active_keeps_same_document() {
        let mut tm = make_n_tabs(4);
        tm.switch_to(2); // active = "tab2"
        tm.pin_tab(0); // pin tab0 — already at idx 0, so move is a no-op.
        assert_eq!(tm.documents[tm.active].title, "tab2");
    }

    #[test]
    fn test_unpinning_active_tab_keeps_same_document_active() {
        let mut tm = make_n_tabs(4);
        tm.pin_tab(0);
        tm.pin_tab(1); // both pinned at indices 0..2
        tm.switch_to(0); // active = "tab0" (pinned)
        tm.unpin_tab(0);
        // "tab0" should now be at index 1 (start of unpinned section) and still active.
        assert_eq!(tm.documents[tm.active].title, "tab0");
        assert_eq!(tm.active, 1);
    }

    #[test]
    fn test_pinned_count_all_pinned() {
        let mut tm = make_n_tabs(3);
        tm.pin_tab(0);
        tm.pin_tab(1);
        tm.pin_tab(2);
        assert_eq!(tm.pinned_count(), 3);
    }

    // ── move_tab ────────────────────────────────────────────────────

    #[test]
    fn test_move_tab_reorders_documents() {
        let mut tm = make_n_tabs(3);
        // Order: [tab0, tab1, tab2] → move 0 to 2 → [tab1, tab2, tab0]
        tm.move_tab(0, 2);
        assert_eq!(tm.documents[0].title, "tab1");
        assert_eq!(tm.documents[1].title, "tab2");
        assert_eq!(tm.documents[2].title, "tab0");
    }

    #[test]
    fn test_move_tab_active_follows_moved_tab() {
        let mut tm = make_n_tabs(3);
        tm.switch_to(0); // active = tab0
        tm.move_tab(0, 2);
        // tab0 is now at index 2 and should still be active.
        assert_eq!(tm.documents[tm.active].title, "tab0");
        assert_eq!(tm.active, 2);
    }

    #[test]
    fn test_move_tab_active_shifts_when_tab_crosses_left_to_right() {
        let mut tm = make_n_tabs(3);
        tm.switch_to(2); // active = tab2
        tm.move_tab(0, 1);
        // tab0 moved from before active to after (on its way past); active should shift left.
        // Order becomes [tab1, tab0, tab2]; tab2 is still active at idx 2.
        assert_eq!(tm.documents[tm.active].title, "tab2");
        assert_eq!(tm.active, 2);
    }

    #[test]
    fn test_move_tab_right_to_left_with_active_at_zero() {
        let mut tm = make_n_tabs(3);
        tm.switch_to(0); // active = tab0
        tm.move_tab(1, 0);
        // Order becomes [tab1, tab0, tab2]; tab0 is now at idx 1.
        assert_eq!(tm.documents[tm.active].title, "tab0");
        assert_eq!(tm.active, 1);
    }

    #[test]
    fn test_move_tab_same_index_is_noop() {
        let mut tm = make_n_tabs(3);
        tm.switch_to(1);
        let before: Vec<_> = tm.documents.iter().map(|d| d.title.clone()).collect();
        tm.move_tab(1, 1);
        let after: Vec<_> = tm.documents.iter().map(|d| d.title.clone()).collect();
        assert_eq!(before, after);
        assert_eq!(tm.active, 1);
    }

    #[test]
    fn test_move_tab_out_of_range_is_noop() {
        let mut tm = make_n_tabs(3);
        let before: Vec<_> = tm.documents.iter().map(|d| d.title.clone()).collect();
        tm.move_tab(99, 0);
        tm.move_tab(0, 99);
        let after: Vec<_> = tm.documents.iter().map(|d| d.title.clone()).collect();
        assert_eq!(before, after);
    }

    #[test]
    fn test_open_from_bytes_equivalent_to_open_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("equiv.txt");
        std::fs::write(&path, "hello\nworld").unwrap();
        let bytes = std::fs::read(&path).unwrap();

        let mut tm1 = TabManager::new();
        tm1.open_file(&path).unwrap();

        let mut tm2 = TabManager::new();
        tm2.open_from_bytes(&path, &bytes).unwrap();

        assert_eq!(
            tm1.active_doc().buffer.to_string(),
            tm2.active_doc().buffer.to_string()
        );
        assert_eq!(tm1.active_doc().encoding, tm2.active_doc().encoding);
        assert_eq!(tm1.active_doc().line_ending, tm2.active_doc().line_ending);
    }

    // ── Split-view: enable / disable ────────────────────────────────

    #[test]
    fn enable_split_single_tab_creates_second_doc() {
        let mut tm = TabManager::new();
        assert_eq!(tm.tab_count(), 1);
        tm.enable_split();
        assert!(tm.is_split());
        assert_eq!(
            tm.tab_count(),
            2,
            "single-tab case should fabricate a second doc so the right pane has content"
        );
        let panes = tm.panes.as_ref().unwrap();
        assert_eq!(panes.left_order, vec![0]);
        assert_eq!(panes.right_order, vec![1]);
        assert_eq!(panes.focused, PaneId::Right);
        assert_eq!(tm.active, 1);
    }

    #[test]
    fn enable_split_multi_tab_partitions_active_into_right() {
        let mut tm = make_n_tabs(4);
        tm.switch_to(2); // active = tab2
        tm.enable_split();
        assert!(tm.is_split());
        let panes = tm.panes.as_ref().unwrap();
        assert_eq!(panes.right_order, vec![2]);
        assert_eq!(panes.left_order, vec![0, 1, 3]);
        assert_eq!(panes.focused, PaneId::Right);
        assert_eq!(panes.right_active, 2);
        assert_eq!(tm.active, 2);
    }

    #[test]
    fn enable_split_is_idempotent() {
        let mut tm = make_n_tabs(3);
        tm.enable_split();
        let snapshot = tm.panes.clone();
        tm.enable_split();
        assert_eq!(
            format!("{:?}", tm.panes),
            format!("{:?}", snapshot),
            "second enable_split should be a no-op"
        );
    }

    #[test]
    fn disable_split_drops_pane_state() {
        let mut tm = make_n_tabs(3);
        tm.enable_split();
        assert!(tm.is_split());
        tm.disable_split();
        assert!(!tm.is_split());
        assert!(tm.panes.is_none());
        // Documents are kept intact.
        assert_eq!(tm.tab_count(), 3);
    }

    // ── Split-view: new_tab / open_file land in focused pane ────────

    #[test]
    fn new_tab_in_split_mode_lands_in_focused_pane() {
        let mut tm = make_n_tabs(2);
        tm.enable_split();
        // After enable_split with 2 tabs, left has [0], right has [1],
        // focused is Right.
        let new_idx_before = tm.tab_count();
        tm.new_tab();
        assert_eq!(tm.tab_count(), new_idx_before + 1);
        let panes = tm.panes.as_ref().unwrap();
        assert!(
            panes.right_order.contains(&new_idx_before),
            "new tab should land in the focused (right) pane"
        );
        assert_eq!(panes.right_active, new_idx_before);
        assert_eq!(tm.active, new_idx_before);
    }

    #[test]
    fn new_tab_in_split_mode_after_left_focus_lands_in_left() {
        let mut tm = make_n_tabs(2);
        tm.enable_split();
        tm.focus_pane(PaneId::Left);
        let new_idx = tm.tab_count();
        tm.new_tab();
        let panes = tm.panes.as_ref().unwrap();
        assert!(panes.left_order.contains(&new_idx));
        assert_eq!(panes.left_active, new_idx);
    }

    // ── Split-view: close_tab auto-collapse ─────────────────────────

    #[test]
    fn close_last_tab_in_pane_auto_collapses_split() {
        // Reproduces the codepath that caused the "two clicks to split"
        // bug — when the right pane only has one tab and that tab is
        // closed, `panes` must drop to None so the UI is consistent.
        let mut tm = make_n_tabs(3);
        tm.enable_split(); // left=[0,1], right=[2], focused=Right
        assert!(tm.is_split());

        // Close the only tab in the right pane.
        tm.close_tab(2);
        assert!(
            !tm.is_split(),
            "split should auto-collapse when a pane becomes empty"
        );
        assert_eq!(tm.tab_count(), 2);
    }

    #[test]
    fn close_non_last_tab_in_pane_keeps_split() {
        let mut tm = make_n_tabs(4);
        tm.switch_to(0);
        tm.enable_split(); // left=[1,2,3], right=[0], focused=Right
                           // Close one of the left pane's tabs (not the last).
        tm.close_tab(2); // closes tab2 (idx 2 in documents)
        assert!(tm.is_split(), "split should stay active");
        let panes = tm.panes.as_ref().unwrap();
        // After removing idx 2, indices > 2 are decremented.
        // Left was [1, 2, 3] → after removing 2 → [1, 2] (3 became 2).
        assert_eq!(panes.left_order, vec![1, 2]);
        assert_eq!(panes.right_order, vec![0]);
    }

    #[test]
    fn close_active_tab_in_pane_picks_survivor() {
        let mut tm = make_n_tabs(4);
        tm.switch_to(0);
        tm.enable_split(); // left=[1,2,3], right=[0]
                           // Switch left pane's active to tab2 (doc idx 2).
        tm.switch_pane_to(PaneId::Left, 2);
        assert_eq!(tm.panes.as_ref().unwrap().left_active, 2);

        // Close the active doc in the left pane.
        tm.close_tab(2);
        assert!(tm.is_split());
        let panes = tm.panes.as_ref().unwrap();
        // Left was [1, 2, 3], left_active was 2. After removing 2:
        // order becomes [1, 2] (3 → 2), and left_active falls back to
        // the new first element.
        assert_eq!(panes.left_order, vec![1, 2]);
        assert!(panes.left_order.contains(&panes.left_active));
    }

    // ── Split-view: move_tab_to_pane ────────────────────────────────

    #[test]
    fn move_tab_to_pane_moves_doc_and_focuses_target() {
        let mut tm = make_n_tabs(4);
        tm.switch_to(0);
        tm.enable_split(); // left=[1,2,3], right=[0], focused=Right
                           // Move tab1 from left to right.
        tm.move_tab_to_pane(1, PaneId::Right);
        assert!(tm.is_split());
        let panes = tm.panes.as_ref().unwrap();
        assert_eq!(panes.left_order, vec![2, 3]);
        assert_eq!(panes.right_order, vec![0, 1]);
        assert_eq!(panes.right_active, 1);
        assert_eq!(panes.focused, PaneId::Right);
        assert_eq!(tm.active, 1);
    }

    #[test]
    fn move_tab_to_pane_collapses_when_source_empty() {
        let mut tm = make_n_tabs(2);
        tm.enable_split(); // left=[0], right=[1]
                           // Move tab0 from left to right — left becomes empty.
        tm.move_tab_to_pane(0, PaneId::Right);
        assert!(
            !tm.is_split(),
            "split should collapse when the source pane empties"
        );
    }

    #[test]
    fn move_tab_to_pane_same_pane_is_noop() {
        let mut tm = make_n_tabs(3);
        tm.enable_split();
        let before = format!("{:?}", tm.panes);
        // Tab2 lives in the right pane (the active doc when split was enabled).
        tm.move_tab_to_pane(2, PaneId::Right);
        let after = format!("{:?}", tm.panes);
        assert_eq!(before, after);
    }

    #[test]
    fn move_tab_to_pane_when_not_split_is_noop() {
        let mut tm = make_n_tabs(3);
        assert!(!tm.is_split());
        tm.move_tab_to_pane(1, PaneId::Right);
        assert!(!tm.is_split(), "no-op should not enable split");
    }

    #[test]
    fn move_tab_to_pane_out_of_range_is_noop() {
        let mut tm = make_n_tabs(2);
        tm.enable_split();
        let before = format!("{:?}", tm.panes);
        tm.move_tab_to_pane(99, PaneId::Right);
        assert_eq!(before, format!("{:?}", tm.panes));
    }

    // ── Split-view: focus_pane / switch_pane_to / switch_to ─────────

    #[test]
    fn focus_pane_updates_active_to_that_panes_doc() {
        let mut tm = make_n_tabs(3);
        tm.switch_to(0);
        tm.enable_split(); // left=[1,2], right=[0], focused=Right
        assert_eq!(tm.active, 0);
        tm.focus_pane(PaneId::Left);
        let panes = tm.panes.as_ref().unwrap();
        assert_eq!(panes.focused, PaneId::Left);
        assert_eq!(tm.active, panes.left_active);
    }

    #[test]
    fn switch_pane_to_only_honours_owned_doc() {
        let mut tm = make_n_tabs(3);
        tm.switch_to(0);
        tm.enable_split(); // left=[1,2], right=[0]
                           // Try to switch the left pane to a doc that lives in the right pane.
        tm.switch_pane_to(PaneId::Left, 0);
        let panes = tm.panes.as_ref().unwrap();
        assert_ne!(
            panes.left_active, 0,
            "left pane should not switch to a right-owned doc"
        );
    }

    #[test]
    fn switch_to_in_split_mode_focuses_owning_pane() {
        let mut tm = make_n_tabs(3);
        tm.switch_to(0);
        tm.enable_split(); // left=[1,2], right=[0], focused=Right
                           // Switch to a doc that lives in the left pane.
        tm.switch_to(1);
        let panes = tm.panes.as_ref().unwrap();
        assert_eq!(panes.focused, PaneId::Left);
        assert_eq!(panes.left_active, 1);
        assert_eq!(tm.active, 1);
    }

    // ── Split-view: move_tab renumbers pane indices ─────────────────

    #[test]
    fn move_tab_in_split_mode_renumbers_pane_orders() {
        let mut tm = make_n_tabs(4);
        tm.switch_to(0);
        tm.enable_split(); // left=[1,2,3], right=[0]
                           // Move doc 3 to position 0 (in front of everything).
        tm.move_tab(3, 0);
        let panes = tm.panes.as_ref().unwrap();
        // Documents now go [tab3, tab0, tab1, tab2]; the pane orders
        // must reflect the new indices but track the same Documents.
        // Originally left held [1,2,3]; after the move tab1 is at idx 2,
        // tab2 at idx 3, tab3 at idx 0, so left should be [2, 3, 0].
        assert_eq!(panes.left_order, vec![2, 3, 0]);
        assert_eq!(panes.right_order, vec![1]); // tab0 is at idx 1 now
    }

    // ── pane_tab_order in single-pane mode ──────────────────────────

    #[test]
    fn pane_tab_order_single_pane_left_returns_all() {
        let tm = make_n_tabs(3);
        assert!(!tm.is_split());
        assert_eq!(tm.pane_tab_order(PaneId::Left), vec![0, 1, 2]);
        assert_eq!(tm.pane_tab_order(PaneId::Right), Vec::<usize>::new());
    }
}
