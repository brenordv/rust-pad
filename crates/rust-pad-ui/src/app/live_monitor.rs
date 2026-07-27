//! Monitors open files for external changes and reloads them.
//!
//! Encapsulates the live-monitoring timer that was previously in `App`.

use std::time::{Duration, Instant};

use crate::io_worker::PendingSave;
use crate::tabs::TabManager;

/// Window after the app's own write during which an mtime increase is treated
/// as the OS settling that write rather than an external edit.
///
/// Windows only guarantees a file's last-write time once the writing handle
/// closes, so a poll landing shortly after our save can read a higher mtime
/// than the one recorded right after the write. See
/// <https://learn.microsoft.com/en-us/windows/win32/sysinfo/file-times>.
const SELF_WRITE_GRACE: Duration = Duration::from_secs(2);

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

    /// Checks all documents with file paths for external changes.
    ///
    /// Live-monitored documents are auto-reloaded (tail behavior).
    /// Non-monitored documents are flagged via `external_change_detected`
    /// so the UI can prompt the user.
    ///
    /// A document with an in-flight save in `pending_saves`, or one whose own
    /// write completed within [`SELF_WRITE_GRACE`], is not treated as
    /// externally changed: the app's own save must never trigger the prompt.
    ///
    /// Only runs at most once per second. When `max_file_size_bytes` is `Some`,
    /// files exceeding the limit are skipped during reload.
    pub fn tick(
        &mut self,
        tabs: &mut TabManager,
        pending_saves: &[PendingSave],
        max_file_size_bytes: Option<u64>,
    ) {
        if self.last_check.elapsed() < Duration::from_secs(1) {
            return;
        }
        for doc in &mut tabs.documents {
            let path = match &doc.file_path {
                Some(p) => p.clone(),
                None => continue,
            };

            // In-flight guard: while our own write to this path is still being
            // written by the I/O worker, its mtime is already bumped but the
            // baseline has not caught up. Skip the whole document; the deferred
            // `mark_saved` will set the correct baseline.
            if pending_saves.iter().any(|s| s.path == path) {
                continue;
            }

            let current_mtime = match std::fs::metadata(&path).and_then(|m| m.modified()) {
                Ok(t) => t,
                Err(_) => continue,
            };
            let changed = match doc.last_known_mtime {
                Some(known) => current_mtime > known,
                None => {
                    if doc.live_monitoring {
                        true
                    } else {
                        // No baseline for non-monitored docs: record mtime and skip.
                        doc.last_known_mtime = Some(current_mtime);
                        false
                    }
                }
            };
            if !changed {
                continue;
            }

            if doc.live_monitoring {
                if let Err(e) = doc.reload_from_disk(max_file_size_bytes) {
                    crate::problem_log::warn_problem(&format!(
                        "Live reload failed for '{}': {e:#}",
                        doc.title
                    ));
                } else {
                    // Scroll to the end of the file (tail behavior)
                    let last_line = doc.buffer.len_lines().saturating_sub(1);
                    doc.scroll_y = last_line as f32;
                    doc.cursor.position = rust_pad_core::cursor::Position::new(last_line, 0);
                    doc.scroll_to_cursor = true;
                }
            } else if !doc.external_change_detected {
                // Post-save grace guard: an mtime increase shortly after our own
                // write is the OS settling that write, not an external edit.
                // Advance the baseline and stay quiet.
                if let Some(written_at) = doc.last_self_write {
                    if written_at.elapsed() < SELF_WRITE_GRACE {
                        tracing::debug!(
                            path = ?path,
                            guard = "grace",
                            elapsed_since_self_write_ms = written_at.elapsed().as_millis() as u64,
                            "live monitor suppressed self-write mtime bump"
                        );
                        doc.last_known_mtime = Some(current_mtime);
                        continue;
                    }
                }

                // Genuine external change: flag for the user prompt.
                let mtime_delta_ms = doc
                    .last_known_mtime
                    .and_then(|known| current_mtime.duration_since(known).ok())
                    .map(|d| d.as_millis() as u64);
                let elapsed_since_self_write_ms =
                    doc.last_self_write.map(|t| t.elapsed().as_millis() as u64);
                tracing::debug!(
                    path = ?path,
                    mtime_delta_ms,
                    elapsed_since_self_write_ms,
                    "live monitor flagged external change"
                );
                doc.external_change_detected = true;
            }
        }
        self.last_check = Instant::now();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_creates_controller() {
        let ctrl = LiveMonitorController::new();
        // Just verify it constructs without panic; last_check is private
        assert!(ctrl.last_check.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn test_tick_skips_non_monitored_docs() {
        let mut ctrl = LiveMonitorController::new();
        // Set last_check far in the past to bypass the 1-second throttle
        ctrl.last_check = Instant::now() - Duration::from_secs(5);
        let mut tabs = TabManager::new();
        assert!(!tabs.active_doc().live_monitoring);
        // Should be a no-op: no panic, no changes
        ctrl.tick(&mut tabs, &[], None);
    }

    #[test]
    fn test_tick_skips_doc_without_filepath() {
        let mut ctrl = LiveMonitorController::new();
        ctrl.last_check = Instant::now() - Duration::from_secs(5);
        let mut tabs = TabManager::new();
        tabs.active_doc_mut().live_monitoring = true;
        // No file_path: tick should skip
        ctrl.tick(&mut tabs, &[], None);
    }

    #[test]
    fn test_tick_throttled_within_one_second() {
        let mut ctrl = LiveMonitorController::new();
        // last_check is just now: tick should not run
        let mut tabs = TabManager::new();
        tabs.active_doc_mut().live_monitoring = true;
        ctrl.tick(&mut tabs, &[], None);
        // No crash, and because of throttling, nothing actually ran
    }

    #[test]
    fn test_tick_detects_file_change() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("test.log");
        std::fs::write(&file, "line1\n").unwrap();

        let mut tabs = TabManager::new();
        tabs.open_file(&file).unwrap();
        tabs.active_doc_mut().live_monitoring = true;
        // Record the mtime so the controller has a baseline
        let mtime = std::fs::metadata(&file).unwrap().modified().unwrap();
        tabs.active_doc_mut().last_known_mtime = Some(mtime);

        // Wait a moment and modify the file
        std::thread::sleep(std::time::Duration::from_millis(50));
        std::fs::write(&file, "line1\nline2\n").unwrap();

        let mut ctrl = LiveMonitorController::new();
        ctrl.last_check = Instant::now() - Duration::from_secs(5);
        ctrl.tick(&mut tabs, &[], None);

        // The document should have been reloaded with new content
        let content = tabs.active_doc().buffer.to_string();
        assert!(
            content.contains("line2"),
            "Expected reloaded content with 'line2', got: {content}"
        );
    }

    #[test]
    fn test_tick_flags_external_change_for_non_monitored_doc() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("readme.txt");
        std::fs::write(&file, "original\n").unwrap();

        let mut tabs = TabManager::new();
        tabs.open_file(&file).unwrap();
        // Not live-monitored: should flag, not auto-reload.
        assert!(!tabs.active_doc().live_monitoring);
        let mtime = std::fs::metadata(&file).unwrap().modified().unwrap();
        tabs.active_doc_mut().last_known_mtime = Some(mtime);

        // Modify the file externally.
        std::thread::sleep(std::time::Duration::from_millis(50));
        std::fs::write(&file, "modified externally\n").unwrap();

        let mut ctrl = LiveMonitorController::new();
        ctrl.last_check = Instant::now() - Duration::from_secs(5);
        ctrl.tick(&mut tabs, &[], None);

        // Should be flagged for user prompt, NOT auto-reloaded.
        assert!(
            tabs.active_doc().external_change_detected,
            "Expected external_change_detected to be true"
        );
        let content = tabs.active_doc().buffer.to_string();
        assert!(
            content.contains("original"),
            "Content should NOT have been reloaded: {content}"
        );
    }

    #[test]
    fn test_tick_does_not_reflag_already_pending_change() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("readme.txt");
        std::fs::write(&file, "original\n").unwrap();

        let mut tabs = TabManager::new();
        tabs.open_file(&file).unwrap();
        let mtime = std::fs::metadata(&file).unwrap().modified().unwrap();
        tabs.active_doc_mut().last_known_mtime = Some(mtime);
        // Pre-flag as pending: tick should not re-flag.
        tabs.active_doc_mut().external_change_detected = true;

        std::thread::sleep(std::time::Duration::from_millis(50));
        std::fs::write(&file, "modified again\n").unwrap();

        let mut ctrl = LiveMonitorController::new();
        ctrl.last_check = Instant::now() - Duration::from_secs(5);
        ctrl.tick(&mut tabs, &[], None);

        // Flag stays true but content unchanged (not reloaded).
        assert!(tabs.active_doc().external_change_detected);
        let content = tabs.active_doc().buffer.to_string();
        assert!(content.contains("original"));
    }

    #[test]
    fn test_tick_records_mtime_baseline_for_non_monitored_without_baseline() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("new.txt");
        std::fs::write(&file, "content\n").unwrap();

        let mut tabs = TabManager::new();
        tabs.open_file(&file).unwrap();
        // Clear the baseline that open_file sets.
        tabs.active_doc_mut().last_known_mtime = None;
        assert!(!tabs.active_doc().live_monitoring);

        let mut ctrl = LiveMonitorController::new();
        ctrl.last_check = Instant::now() - Duration::from_secs(5);
        ctrl.tick(&mut tabs, &[], None);

        // Should record the baseline, not flag as changed.
        assert!(!tabs.active_doc().external_change_detected);
        assert!(tabs.active_doc().last_known_mtime.is_some());
    }

    #[test]
    fn test_tick_skips_doc_with_in_flight_save() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("doc.txt");
        std::fs::write(&file, "original\n").unwrap();

        let mut tabs = TabManager::new();
        tabs.open_file(&file).unwrap();
        let baseline = std::fs::metadata(&file).unwrap().modified().unwrap();
        tabs.active_doc_mut().last_known_mtime = Some(baseline);

        // Our own save (still in flight on the I/O worker) bumps the mtime.
        std::thread::sleep(std::time::Duration::from_millis(50));
        std::fs::write(&file, "our own save\n").unwrap();

        let pending = vec![PendingSave {
            path: file.clone(),
            content_version: 0,
        }];

        let mut ctrl = LiveMonitorController::new();
        ctrl.last_check = Instant::now() - Duration::from_secs(5);
        ctrl.tick(&mut tabs, &pending, None);

        assert!(
            !tabs.active_doc().external_change_detected,
            "an in-flight save must not be flagged as an external change"
        );
        assert_eq!(
            tabs.active_doc().last_known_mtime,
            Some(baseline),
            "the baseline must be left untouched for the pending mark_saved"
        );
    }

    #[test]
    fn test_tick_grace_window_suppresses_and_rebaselines_self_write() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("doc.txt");
        std::fs::write(&file, "content\n").unwrap();

        let mut tabs = TabManager::new();
        tabs.open_file(&file).unwrap();
        let current = std::fs::metadata(&file).unwrap().modified().unwrap();
        {
            let doc = tabs.active_doc_mut();
            // Force `changed == true`: baseline well behind the on-disk mtime,
            // mimicking the OS settling our write to a higher value than the
            // one mark_saved recorded.
            doc.last_known_mtime = Some(current - Duration::from_secs(10));
            // We wrote this file ourselves, just now (within the grace window).
            doc.last_self_write = Some(Instant::now());
        }

        let mut ctrl = LiveMonitorController::new();
        ctrl.last_check = Instant::now() - Duration::from_secs(5);
        ctrl.tick(&mut tabs, &[], None);

        assert!(
            !tabs.active_doc().external_change_detected,
            "a self-write inside the grace window must not be flagged"
        );
        assert_eq!(
            tabs.active_doc().last_known_mtime,
            Some(current),
            "the baseline should advance to the current mtime"
        );
    }
}
