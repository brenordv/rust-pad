/// Tab manager for handling multiple open documents.
use std::sync::Arc;

use rust_pad_core::document::Document;
use rust_pad_core::history::{HistoryConfig, PersistenceLayer};

/// Manages open document tabs.
#[derive(Debug)]
pub struct TabManager {
    /// All open documents.
    pub documents: Vec<Document>,
    /// Index of the active document.
    pub active: usize,
    /// Shared persistence layer for undo history (None = in-memory only).
    persistence: Option<Arc<PersistenceLayer>>,
    /// History configuration.
    config: HistoryConfig,
    /// Default file extension for new untitled tabs (e.g. "txt", "md"). Empty = none.
    pub default_extension: String,
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
            persistence: None,
            config: HistoryConfig::default(),
            default_extension: String::new(),
        }
    }

    /// Creates a new tab manager with persistent undo history.
    pub fn with_persistence(persistence: Arc<PersistenceLayer>, config: HistoryConfig) -> Self {
        let doc = Document::with_persistence(Arc::clone(&persistence), &config);
        Self {
            documents: vec![doc],
            active: 0,
            persistence: Some(persistence),
            config,
            default_extension: String::new(),
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
        self.documents.push(doc);
        self.active = self.documents.len() - 1;
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
        // Check if this file is already open
        for (idx, doc) in self.documents.iter().enumerate() {
            if doc.file_path.as_deref() == Some(path) {
                self.active = idx;
                return Ok(());
            }
        }

        let doc = match &self.persistence {
            Some(pl) => Document::open_with_persistence(path, Arc::clone(pl), &self.config)?,
            None => Document::open(path)?,
        };
        self.documents.push(doc);
        self.active = self.documents.len() - 1;
        Ok(())
    }

    /// Closes the active tab. Returns true if the tab was closed.
    /// The caller should check for unsaved changes before calling this.
    pub fn close_active(&mut self) -> bool {
        if self.documents.len() <= 1 {
            self.delete_tab_history(0);
            self.documents[0] = self.create_document();
            self.active = 0;
            return true;
        }

        self.delete_tab_history(self.active);
        self.documents.remove(self.active);
        if self.active >= self.documents.len() {
            self.active = self.documents.len() - 1;
        }
        true
    }

    /// Closes a tab by index. Returns true if closed.
    pub fn close_tab(&mut self, idx: usize) -> bool {
        if idx >= self.documents.len() {
            return false;
        }

        if self.documents.len() <= 1 {
            self.delete_tab_history(0);
            self.documents[0] = self.create_document();
            self.active = 0;
            return true;
        }

        self.delete_tab_history(idx);
        self.documents.remove(idx);
        if self.active >= self.documents.len() {
            self.active = self.documents.len() - 1;
        } else if self.active > idx {
            self.active -= 1;
        }
        true
    }

    /// Switches to a specific tab.
    pub fn switch_to(&mut self, idx: usize) {
        if idx < self.documents.len() {
            self.active = idx;
        }
    }

    /// Returns the number of open tabs.
    pub fn tab_count(&self) -> usize {
        self.documents.len()
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
        match &self.persistence {
            Some(pl) => Document::with_persistence(Arc::clone(pl), &self.config),
            None => Document::new(),
        }
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
}
