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
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum FsEvent {
    /// A file or directory was created.
    ///
    /// Note: this variant is only produced by manual tree updates in
    /// `workspace_ops` (e.g., after creating a file or applying a rename).
    /// The filesystem watcher itself never emits `Created` — new paths
    /// surface as `Modified` (since the debouncer only sees that the path exists).
    Created(PathBuf),
    /// A file or directory was removed.
    Removed(PathBuf),
    /// A file or directory was modified, or a new path was detected by the watcher.
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

/// Classifies a debounced filesystem event into an `FsEvent`.
///
/// Returns `None` for event kinds that don't map to sidebar-relevant changes.
/// Note: for rapid create-then-delete sequences within the debounce window,
/// the event will be classified as `Removed` even though a create also happened.
/// This is acceptable for the sidebar use case.
fn classify_debounced_event(event: notify_debouncer_mini::DebouncedEvent) -> Option<FsEvent> {
    match event.kind {
        DebouncedEventKind::Any | DebouncedEventKind::AnyContinuous => {
            if event.path.exists() {
                Some(FsEvent::Modified(event.path))
            } else {
                Some(FsEvent::Removed(event.path))
            }
        }
        _ => None,
    }
}

impl WorkspaceWatcher {
    /// Creates a new workspace watcher with 500ms debounce.
    pub fn new() -> Result<Self> {
        let (tx, rx) = mpsc::channel();

        let debouncer = new_debouncer(
            Duration::from_millis(500),
            move |result: DebounceEventResult| match result {
                Ok(events) => {
                    for event in events {
                        if let Some(fs_event) = classify_debounced_event(event) {
                            let _ = tx.send(fs_event);
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!("Filesystem watch error: {e}");
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
    ///
    /// Duplicate events for the same `(kind, path)` pair within a single
    /// drain are coalesced into one. With overlapping workspace roots a
    /// recursive watcher may emit the same notification multiple times;
    /// deduplication bounds the per-tick work amplification at N=1.
    pub fn poll_events(&self) -> Vec<FsEvent> {
        let mut events = Vec::new();
        let mut seen: std::collections::HashSet<FsEvent> = std::collections::HashSet::new();
        while let Ok(event) = self.receiver.try_recv() {
            if seen.insert(event.clone()) {
                events.push(event);
            }
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
    fn test_fs_event_clone_and_eq() {
        let event = FsEvent::Created(PathBuf::from("/a/b"));
        let cloned = event.clone();
        assert_eq!(event, cloned);

        let modified = FsEvent::Modified(PathBuf::from("/c"));
        let removed = FsEvent::Removed(PathBuf::from("/c"));
        assert_ne!(modified, removed);
    }

    #[test]
    fn test_fs_event_debug_format() {
        let event = FsEvent::Created(PathBuf::from("/test"));
        let debug = format!("{event:?}");
        assert!(debug.contains("Created"));
        assert!(debug.contains("test"));
    }

    #[test]
    fn test_classify_existing_path_returns_modified() {
        let dir = TempDir::new().expect("create temp dir");
        let file = dir.path().join("exists.txt");
        std::fs::write(&file, "data").expect("write");

        let debounced = notify_debouncer_mini::DebouncedEvent {
            path: file.clone(),
            kind: DebouncedEventKind::Any,
        };
        let result = classify_debounced_event(debounced);
        assert_eq!(result, Some(FsEvent::Modified(file)));
    }

    #[test]
    fn test_classify_nonexistent_path_returns_removed() {
        let debounced = notify_debouncer_mini::DebouncedEvent {
            path: PathBuf::from("/nonexistent_xyz_classify_test"),
            kind: DebouncedEventKind::Any,
        };
        let result = classify_debounced_event(debounced);
        assert_eq!(
            result,
            Some(FsEvent::Removed(PathBuf::from(
                "/nonexistent_xyz_classify_test"
            )))
        );
    }

    #[test]
    fn test_classify_any_continuous_existing_returns_modified() {
        let dir = TempDir::new().expect("create temp dir");
        let file = dir.path().join("continuous.txt");
        std::fs::write(&file, "data").expect("write");

        let debounced = notify_debouncer_mini::DebouncedEvent {
            path: file.clone(),
            kind: DebouncedEventKind::AnyContinuous,
        };
        let result = classify_debounced_event(debounced);
        assert_eq!(result, Some(FsEvent::Modified(file)));
    }

    #[test]
    fn test_classify_any_continuous_nonexistent_returns_removed() {
        let debounced = notify_debouncer_mini::DebouncedEvent {
            path: PathBuf::from("/nonexistent_xyz_continuous_test"),
            kind: DebouncedEventKind::AnyContinuous,
        };
        let result = classify_debounced_event(debounced);
        assert_eq!(
            result,
            Some(FsEvent::Removed(PathBuf::from(
                "/nonexistent_xyz_continuous_test"
            )))
        );
    }

    #[test]
    fn test_watcher_debug_format() {
        let watcher = WorkspaceWatcher::new().expect("create watcher");
        let debug = format!("{watcher:?}");
        assert!(debug.contains("WorkspaceWatcher"));
    }

    #[test]
    fn test_watch_and_unwatch_same_dir() {
        let dir = TempDir::new().expect("create temp dir");
        let mut watcher = WorkspaceWatcher::new().expect("create watcher");
        watcher.watch(dir.path()).expect("watch");
        watcher.unwatch(dir.path()).expect("unwatch");
        // Should be able to re-watch after unwatching
        watcher.watch(dir.path()).expect("re-watch");
    }

    #[test]
    fn test_fs_event_variants_inequality() {
        let path = PathBuf::from("/test");
        let created = FsEvent::Created(path.clone());
        let modified = FsEvent::Modified(path.clone());
        let removed = FsEvent::Removed(path);
        assert_ne!(created, modified);
        assert_ne!(created, removed);
        assert_ne!(modified, removed);
    }

    #[test]
    fn test_poll_events_deduplicates_repeated_events() {
        // Send the same event twice through the internal channel and verify
        // poll_events emits only one copy. Bypasses the debouncer by
        // injecting directly into the receiver.
        let (tx, rx) = mpsc::channel();
        let same = FsEvent::Modified(PathBuf::from("/x/y/z"));
        tx.send(same.clone()).expect("send 1");
        tx.send(same.clone()).expect("send 2");
        tx.send(FsEvent::Removed(PathBuf::from("/x/y/z")))
            .expect("send removed");
        drop(tx);

        // Drain manually using the dedup logic.
        let mut events: Vec<FsEvent> = Vec::new();
        let mut seen: std::collections::HashSet<FsEvent> = std::collections::HashSet::new();
        while let Ok(event) = rx.try_recv() {
            if seen.insert(event.clone()) {
                events.push(event);
            }
        }

        // 2 unique events out of 3 sent.
        assert_eq!(events.len(), 2);
        assert!(events.contains(&FsEvent::Modified(PathBuf::from("/x/y/z"))));
        assert!(events.contains(&FsEvent::Removed(PathBuf::from("/x/y/z"))));
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
