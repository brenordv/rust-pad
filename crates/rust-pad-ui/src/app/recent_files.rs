//! Manages the recent files list: tracking, cleanup, and persistence.
//!
//! Encapsulates all recent-files state that was previously spread across `App`.

use std::path::{Path, PathBuf};

use rust_pad_config::RecentFilesCleanup;

/// Owns all recent-files state for the application.
pub struct RecentFilesManager {
    /// Whether the recent files feature is enabled.
    pub enabled: bool,
    /// Maximum number of recent files to remember.
    pub max_count: usize,
    /// When to prune dead files from the recent list.
    pub cleanup: RecentFilesCleanup,
    /// Most-recently-opened file paths (most recent first).
    pub files: Vec<PathBuf>,
}

impl RecentFilesManager {
    /// Creates a new manager from config values, applying startup cleanup if configured.
    pub fn new(
        enabled: bool,
        max_count: usize,
        cleanup: RecentFilesCleanup,
        files: Vec<String>,
    ) -> Self {
        let mut paths: Vec<PathBuf> = files.iter().map(PathBuf::from).collect();
        if matches!(
            cleanup,
            RecentFilesCleanup::OnStartup | RecentFilesCleanup::Both
        ) {
            paths.retain(|p| p.is_file());
        }
        Self {
            enabled,
            max_count,
            cleanup,
            files: paths,
        }
    }

    /// Adds a path to the recent files list, deduplicating and capping at max count.
    pub fn track(&mut self, path: &Path) {
        if !self.enabled {
            return;
        }
        let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        self.files.retain(|p| p != &canonical);
        self.files.insert(0, canonical);
        self.files.truncate(self.max_count);
    }

    /// Removes dead (non-existent) files if the cleanup mode requires it on menu open.
    pub fn cleanup_on_menu_open(&mut self) {
        if matches!(
            self.cleanup,
            RecentFilesCleanup::OnMenuOpen | RecentFilesCleanup::Both
        ) {
            self.files.retain(|p| p.is_file());
        }
    }

    /// Returns the file list as owned strings for config serialization.
    pub fn to_config_strings(&self) -> Vec<String> {
        self.files
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── new() ───────────────────────────────────────────────────────

    #[test]
    fn test_new_stores_fields() {
        let mgr = RecentFilesManager::new(true, 5, RecentFilesCleanup::OnMenuOpen, Vec::new());
        assert!(mgr.enabled);
        assert_eq!(mgr.max_count, 5);
        assert_eq!(mgr.cleanup, RecentFilesCleanup::OnMenuOpen);
        assert!(mgr.files.is_empty());
    }

    #[test]
    fn test_new_on_startup_cleanup_removes_nonexistent() {
        // Non-existent paths are pruned when cleanup is OnStartup
        let mgr = RecentFilesManager::new(
            true,
            10,
            RecentFilesCleanup::OnStartup,
            vec![
                "/nonexistent/a.txt".to_string(),
                "/nonexistent/b.txt".to_string(),
            ],
        );
        assert!(mgr.files.is_empty());
    }

    #[test]
    fn test_new_on_startup_cleanup_with_both() {
        let mgr = RecentFilesManager::new(
            true,
            10,
            RecentFilesCleanup::Both,
            vec!["/nonexistent/file.rs".to_string()],
        );
        assert!(mgr.files.is_empty());
    }

    #[test]
    fn test_new_on_menu_open_keeps_nonexistent() {
        // OnMenuOpen does NOT prune at construction time
        let mgr = RecentFilesManager::new(
            true,
            10,
            RecentFilesCleanup::OnMenuOpen,
            vec!["/nonexistent/file.rs".to_string()],
        );
        assert_eq!(mgr.files.len(), 1);
    }

    #[test]
    fn test_new_with_real_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("test.txt");
        std::fs::write(&file, "hello").unwrap();

        let mgr = RecentFilesManager::new(
            true,
            10,
            RecentFilesCleanup::OnStartup,
            vec![file.to_string_lossy().into_owned()],
        );
        assert_eq!(mgr.files.len(), 1);
    }

    // ── track() ─────────────────────────────────────────────────────

    #[test]
    fn test_track_adds_path() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("test.txt");
        std::fs::write(&file, "hello").unwrap();

        let mut mgr = RecentFilesManager::new(true, 10, RecentFilesCleanup::OnMenuOpen, Vec::new());
        mgr.track(&file);
        assert_eq!(mgr.files.len(), 1);
    }

    #[test]
    fn test_track_deduplicates() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("test.txt");
        std::fs::write(&file, "hello").unwrap();

        let mut mgr = RecentFilesManager::new(true, 10, RecentFilesCleanup::OnMenuOpen, Vec::new());
        mgr.track(&file);
        mgr.track(&file);
        assert_eq!(mgr.files.len(), 1);
    }

    #[test]
    fn test_track_moves_to_front() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.txt");
        let b = dir.path().join("b.txt");
        std::fs::write(&a, "a").unwrap();
        std::fs::write(&b, "b").unwrap();

        let mut mgr = RecentFilesManager::new(true, 10, RecentFilesCleanup::OnMenuOpen, Vec::new());
        mgr.track(&a);
        mgr.track(&b);
        // b was tracked last, so it should be first
        assert!(mgr.files[0].ends_with("b.txt"));
        // Re-track a — it should move back to front
        mgr.track(&a);
        assert!(mgr.files[0].ends_with("a.txt"));
        assert_eq!(mgr.files.len(), 2);
    }

    #[test]
    fn test_track_caps_at_max_count() {
        let dir = tempfile::tempdir().unwrap();
        let mut mgr = RecentFilesManager::new(true, 3, RecentFilesCleanup::OnMenuOpen, Vec::new());
        for i in 0..5 {
            let file = dir.path().join(format!("{i}.txt"));
            std::fs::write(&file, "x").unwrap();
            mgr.track(&file);
        }
        assert_eq!(mgr.files.len(), 3);
    }

    #[test]
    fn test_track_disabled_is_noop() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("test.txt");
        std::fs::write(&file, "hello").unwrap();

        let mut mgr =
            RecentFilesManager::new(false, 10, RecentFilesCleanup::OnMenuOpen, Vec::new());
        mgr.track(&file);
        assert!(mgr.files.is_empty());
    }

    // ── cleanup_on_menu_open() ──────────────────────────────────────

    #[test]
    fn test_cleanup_on_menu_open_removes_dead_files() {
        let mut mgr = RecentFilesManager::new(true, 10, RecentFilesCleanup::OnMenuOpen, Vec::new());
        mgr.files.push(PathBuf::from("/nonexistent/dead_file.txt"));
        assert_eq!(mgr.files.len(), 1);
        mgr.cleanup_on_menu_open();
        assert!(mgr.files.is_empty());
    }

    #[test]
    fn test_cleanup_on_menu_open_keeps_existing_files() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("alive.txt");
        std::fs::write(&file, "alive").unwrap();

        let mut mgr = RecentFilesManager::new(true, 10, RecentFilesCleanup::OnMenuOpen, Vec::new());
        mgr.files.push(file);
        mgr.cleanup_on_menu_open();
        assert_eq!(mgr.files.len(), 1);
    }

    #[test]
    fn test_cleanup_on_menu_open_noop_for_on_startup() {
        let mut mgr = RecentFilesManager::new(true, 10, RecentFilesCleanup::OnStartup, Vec::new());
        mgr.files.push(PathBuf::from("/nonexistent/dead_file.txt"));
        mgr.cleanup_on_menu_open();
        // OnStartup does NOT clean on menu open
        assert_eq!(mgr.files.len(), 1);
    }

    #[test]
    fn test_cleanup_on_menu_open_works_with_both() {
        let mut mgr = RecentFilesManager::new(true, 10, RecentFilesCleanup::Both, Vec::new());
        mgr.files.push(PathBuf::from("/nonexistent/dead_file.txt"));
        mgr.cleanup_on_menu_open();
        assert!(mgr.files.is_empty());
    }

    // ── to_config_strings() ─────────────────────────────────────────

    #[test]
    fn test_to_config_strings_empty() {
        let mgr = RecentFilesManager::new(true, 10, RecentFilesCleanup::OnMenuOpen, Vec::new());
        assert!(mgr.to_config_strings().is_empty());
    }

    #[test]
    fn test_to_config_strings_round_trip() {
        let mut mgr = RecentFilesManager::new(true, 10, RecentFilesCleanup::OnMenuOpen, Vec::new());
        mgr.files.push(PathBuf::from("/tmp/a.txt"));
        mgr.files.push(PathBuf::from("/tmp/b.rs"));
        let strings = mgr.to_config_strings();
        assert_eq!(strings.len(), 2);
        assert_eq!(strings[0], "/tmp/a.txt");
        assert_eq!(strings[1], "/tmp/b.rs");
    }
}
