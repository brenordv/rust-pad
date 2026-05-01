//! Manages periodic auto-saving of file-backed documents.
//!
//! Encapsulates the auto-save timer and settings that were previously in `App`.

use std::time::{Duration, Instant};

use crate::tabs::TabManager;

/// Owns auto-save state for the application.
pub struct AutoSaveController {
    /// Whether auto-save is enabled.
    pub enabled: bool,
    /// Interval in seconds between auto-saves (minimum 5).
    pub interval_secs: u64,
    /// Last time auto-save ran.
    last_save: Instant,
}

impl AutoSaveController {
    /// Creates a new controller from config values.
    pub fn new(enabled: bool, interval_secs: u64) -> Self {
        Self {
            enabled,
            interval_secs,
            last_save: Instant::now(),
        }
    }

    /// Checks if it's time to auto-save and saves all modified file-backed documents.
    ///
    /// Returns `None` when auto-save was skipped (disabled or interval not
    /// elapsed), or `Some(errors)` when it ran. The error vec contains
    /// human-readable messages for any documents that failed to save.
    pub fn tick(&mut self, tabs: &mut TabManager) -> Option<Vec<String>> {
        if !self.enabled {
            return None;
        }
        if self.last_save.elapsed() < Duration::from_secs(self.interval_secs) {
            return None;
        }
        let mut errors = Vec::new();
        for doc in &mut tabs.documents {
            if doc.modified && doc.file_path.is_some() {
                if let Err(e) = doc.save() {
                    let msg = format!("Auto-save failed for '{}': {e:#}", doc.title);
                    tracing::warn!("{msg}");
                    errors.push(msg);
                }
            }
        }
        self.last_save = Instant::now();
        Some(errors)
    }

    /// Returns the auto-save interval for repaint scheduling.
    pub fn repaint_interval(&self) -> Option<Duration> {
        if self.enabled {
            Some(Duration::from_secs(self.interval_secs))
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── new() ───────────────────────────────────────────────────────

    #[test]
    fn test_new_stores_fields() {
        let ctrl = AutoSaveController::new(true, 60);
        assert!(ctrl.enabled);
        assert_eq!(ctrl.interval_secs, 60);
    }

    #[test]
    fn test_new_disabled() {
        let ctrl = AutoSaveController::new(false, 30);
        assert!(!ctrl.enabled);
    }

    // ── repaint_interval() ──────────────────────────────────────────

    #[test]
    fn test_repaint_interval_when_enabled() {
        let ctrl = AutoSaveController::new(true, 45);
        assert_eq!(ctrl.repaint_interval(), Some(Duration::from_secs(45)));
    }

    #[test]
    fn test_repaint_interval_when_disabled() {
        let ctrl = AutoSaveController::new(false, 45);
        assert_eq!(ctrl.repaint_interval(), None);
    }

    // ── tick() ──────────────────────────────────────────────────────

    #[test]
    fn test_tick_returns_none_when_disabled() {
        let mut ctrl = AutoSaveController::new(false, 0);
        let mut tabs = TabManager::new();
        assert!(ctrl.tick(&mut tabs).is_none());
    }

    #[test]
    fn test_tick_returns_none_before_interval_elapsed() {
        let mut ctrl = AutoSaveController::new(true, 3600);
        let mut tabs = TabManager::new();
        // Interval is 1 hour — tick should return None immediately
        assert!(ctrl.tick(&mut tabs).is_none());
    }

    #[test]
    fn test_tick_returns_some_when_interval_elapsed() {
        let mut ctrl = AutoSaveController::new(true, 0);
        let mut tabs = TabManager::new();
        // interval_secs=0 means the interval has already elapsed
        assert!(ctrl.tick(&mut tabs).is_some());
    }

    #[test]
    fn test_tick_saves_modified_file_backed_doc() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("test.txt");
        std::fs::write(&file, "original").unwrap();

        let mut tabs = TabManager::new();
        tabs.open_file(&file).unwrap();
        let doc = tabs.active_doc_mut();
        doc.insert_text(" modified");
        assert!(doc.modified);

        let mut ctrl = AutoSaveController::new(true, 0);
        let result = ctrl.tick(&mut tabs);
        assert!(result.is_some());
        // Document should no longer be marked as modified after save
        assert!(!tabs.active_doc().modified);
    }

    #[test]
    fn test_tick_skips_unmodified_doc() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("test.txt");
        std::fs::write(&file, "content").unwrap();

        let mut tabs = TabManager::new();
        tabs.open_file(&file).unwrap();
        assert!(!tabs.active_doc().modified);

        let mut ctrl = AutoSaveController::new(true, 0);
        let result = ctrl.tick(&mut tabs);
        // tick returns Some (it ran), but nothing was actually saved
        assert!(result.is_some());
        assert!(!tabs.active_doc().modified);
    }

    #[test]
    fn test_tick_skips_unsaved_doc_without_filepath() {
        let mut tabs = TabManager::new();
        tabs.active_doc_mut().insert_text("unsaved content");
        tabs.active_doc_mut().modified = true;

        let mut ctrl = AutoSaveController::new(true, 0);
        ctrl.tick(&mut tabs);
        // Doc has no file_path, so it should still be modified
        assert!(tabs.active_doc().modified);
    }

    #[test]
    fn test_tick_resets_timer() {
        let mut ctrl = AutoSaveController::new(true, 0);
        let mut tabs = TabManager::new();
        // First tick succeeds (interval=0 means always elapsed)
        assert!(ctrl.tick(&mut tabs).is_some());
        // Second tick with interval_secs=3600 should skip (timer just reset)
        ctrl.interval_secs = 3600;
        assert!(ctrl.tick(&mut tabs).is_none());
    }
}
