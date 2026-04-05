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
    /// Returns `true` if auto-save was performed.
    pub fn tick(&mut self, tabs: &mut TabManager) -> bool {
        if !self.enabled {
            return false;
        }
        if self.last_save.elapsed() < Duration::from_secs(self.interval_secs) {
            return false;
        }
        for doc in &mut tabs.documents {
            if doc.modified && doc.file_path.is_some() {
                if let Err(e) = doc.save() {
                    tracing::warn!("Auto-save failed for '{}': {e:#}", doc.title);
                }
            }
        }
        self.last_save = Instant::now();
        true
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
