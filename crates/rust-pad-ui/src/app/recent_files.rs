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
