/// Filesystem watcher for workspace directories.
///
/// Wraps the `notify` crate to provide debounced filesystem events
/// that the sidebar can use to update the tree incrementally.
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use anyhow::{Context, Result};
use notify::{RecommendedWatcher, RecursiveMode};
use notify_debouncer_mini::{new_debouncer, DebounceEventResult, DebouncedEventKind, Debouncer};

/// A filesystem event relevant to the workspace sidebar.
///
/// Note: the original plan included a `Renamed { from, to }` variant, but
/// `notify-debouncer-mini` cannot reliably detect renames — they surface as
/// a `Removed` followed by a `Created`, which is sufficient for the sidebar
/// tree's incremental update logic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FsEvent {
    /// A file or directory was created.
    Created(PathBuf),
    /// A file or directory was removed.
    Removed(PathBuf),
    /// A file was modified (content change).
    Modified(PathBuf),
}

/// Watches workspace directories for filesystem changes.
///
/// Uses `notify` with debouncing (~500ms) to avoid event floods
/// during bulk operations.
pub struct WorkspaceWatcher {
    _debouncer: Debouncer<RecommendedWatcher>,
    receiver: Receiver<FsEvent>,
}

impl std::fmt::Debug for WorkspaceWatcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WorkspaceWatcher").finish()
    }
}

impl WorkspaceWatcher {
    /// Creates a new workspace watcher with 500ms debounce.
    pub fn new() -> Result<Self> {
        let (tx, rx) = mpsc::channel();

        let debouncer = new_debouncer(
            Duration::from_millis(500),
            move |result: DebounceEventResult| {
                match result {
                    Ok(events) => {
                        for event in events {
                            let fs_event = match event.kind {
                                DebouncedEventKind::Any | DebouncedEventKind::AnyContinuous => {
                                    // We can't distinguish create/remove/modify from debounced
                                    // events alone — the path existence tells us if it was
                                    // created or removed.
                                    // Note: for rapid create-then-delete sequences within the
                                    // debounce window, the event will be classified as Removed
                                    // even though a create also happened. This is acceptable
                                    // for the sidebar use case.
                                    if event.path.exists() {
                                        FsEvent::Modified(event.path)
                                    } else {
                                        FsEvent::Removed(event.path)
                                    }
                                }
                                _ => continue,
                            };
                            let _ = tx.send(fs_event);
                        }
                    }
                    Err(e) => {
                        tracing::warn!("Filesystem watch error: {e}");
                    }
                }
            },
        )
        .context("Failed to create filesystem debouncer")?;

        Ok(Self {
            _debouncer: debouncer,
            receiver: rx,
        })
    }

    /// Starts recursive watching on a directory.
    pub fn watch(&mut self, path: &Path) -> Result<()> {
        self._debouncer
            .watcher()
            .watch(path, RecursiveMode::Recursive)
            .with_context(|| format!("Failed to watch directory: {}", path.display()))
    }

    /// Stops watching a directory.
    pub fn unwatch(&mut self, path: &Path) -> Result<()> {
        self._debouncer
            .watcher()
            .unwatch(path)
            .with_context(|| format!("Failed to unwatch directory: {}", path.display()))
    }

    /// Non-blocking drain of all pending filesystem events.
    pub fn poll_events(&self) -> Vec<FsEvent> {
        let mut events = Vec::new();
        while let Ok(event) = self.receiver.try_recv() {
            events.push(event);
        }
        events
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_watcher_creation() {
        let watcher = WorkspaceWatcher::new();
        assert!(watcher.is_ok(), "Watcher creation should not fail");
    }

    #[test]
    fn test_watch_temp_directory() {
        let dir = TempDir::new().expect("create temp dir");
        let mut watcher = WorkspaceWatcher::new().expect("create watcher");
        let result = watcher.watch(dir.path());
        assert!(result.is_ok(), "Watching a temp directory should succeed");
    }

    #[test]
    fn test_unwatch_watched_directory() {
        let dir = TempDir::new().expect("create temp dir");
        let mut watcher = WorkspaceWatcher::new().expect("create watcher");
        watcher.watch(dir.path()).expect("watch");
        let result = watcher.unwatch(dir.path());
        assert!(result.is_ok(), "Unwatching should succeed");
    }

    #[test]
    fn test_poll_empty_returns_empty() {
        let watcher = WorkspaceWatcher::new().expect("create watcher");
        let events = watcher.poll_events();
        assert!(events.is_empty());
    }

    #[test]
    fn test_file_creation_produces_event() {
        let dir = TempDir::new().expect("create temp dir");
        let mut watcher = WorkspaceWatcher::new().expect("create watcher");
        watcher.watch(dir.path()).expect("watch");

        // Create a file in the watched directory
        let file_path = dir.path().join("test.txt");
        std::fs::write(&file_path, "hello").expect("write file");

        // Wait for debounce period + some margin
        std::thread::sleep(Duration::from_millis(800));

        let events = watcher.poll_events();
        // We should get at least one event for the created file
        assert!(
            !events.is_empty(),
            "Should receive events after file creation"
        );
        // The event should reference our file (as Modified since it exists now)
        let has_our_file = events.iter().any(|e| match e {
            FsEvent::Modified(p) | FsEvent::Created(p) => p == &file_path,
            _ => false,
        });
        assert!(has_our_file, "Events should reference the created file");
    }

    #[test]
    fn test_file_deletion_produces_removed_event() {
        let dir = TempDir::new().expect("create temp dir");
        let file_path = dir.path().join("to_delete.txt");
        std::fs::write(&file_path, "data").expect("write");

        let mut watcher = WorkspaceWatcher::new().expect("create watcher");
        watcher.watch(dir.path()).expect("watch");

        // Drain any initial events
        std::thread::sleep(Duration::from_millis(600));
        let _ = watcher.poll_events();

        // Delete the file
        std::fs::remove_file(&file_path).expect("delete");

        // Wait for debounce
        std::thread::sleep(Duration::from_millis(800));

        let events = watcher.poll_events();
        let has_removed = events
            .iter()
            .any(|e| matches!(e, FsEvent::Removed(p) if p == &file_path));
        assert!(
            has_removed,
            "Should receive Removed event after file deletion, got: {events:?}"
        );
    }
}
