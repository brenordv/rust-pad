//! Monitors open files for external changes and reloads them.
//!
//! Encapsulates the live-monitoring timer that was previously in `App`.

use std::time::{Duration, Instant};

use crate::tabs::TabManager;

/// Owns the live file monitoring timer for the application.
pub struct LiveMonitorController {
    /// Last time we checked for file changes.
    last_check: Instant,
}

impl LiveMonitorController {
    /// Creates a new controller.
    pub fn new() -> Self {
        Self {
            last_check: Instant::now(),
        }
    }

    /// Checks all live-monitored documents for external file changes and reloads them.
    ///
    /// Only runs at most once per second.
    pub fn tick(&mut self, tabs: &mut TabManager) {
        if self.last_check.elapsed() < Duration::from_secs(1) {
            return;
        }
        for doc in &mut tabs.documents {
            if !doc.live_monitoring {
                continue;
            }
            let path = match &doc.file_path {
                Some(p) => p.clone(),
                None => continue,
            };
            let current_mtime = match std::fs::metadata(&path).and_then(|m| m.modified()) {
                Ok(t) => t,
                Err(_) => continue,
            };
            let changed = match doc.last_known_mtime {
                Some(known) => current_mtime > known,
                None => true,
            };
            if changed {
                if let Err(e) = doc.reload_from_disk() {
                    tracing::warn!("Live reload failed for '{}': {e:#}", doc.title);
                } else {
                    // Scroll to the end of the file (tail behavior)
                    let last_line = doc.buffer.len_lines().saturating_sub(1);
                    doc.scroll_y = last_line as f32;
                    doc.cursor.position = rust_pad_core::cursor::Position::new(last_line, 0);
                    doc.scroll_to_cursor = true;
                }
            }
        }
        self.last_check = Instant::now();
    }
}
