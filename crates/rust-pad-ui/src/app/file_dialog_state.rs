//! Manages file dialog preferences: last-used folder, default folder, and default extension.
//!
//! Encapsulates all file-dialog-related fields that were previously in `App`.

use std::path::{Path, PathBuf};

/// Owns file-dialog preferences for the application.
pub struct FileDialogState {
    /// Whether to remember the last folder used in open/save dialogs.
    pub remember_last_folder: bool,
    /// Default working folder for file dialogs. Empty = user's home directory.
    pub default_work_folder: String,
    /// Last folder used in an open/save dialog (persisted across sessions).
    pub last_used_folder: Option<PathBuf>,
    /// Default file extension for new untitled tabs (e.g. "txt", "md"). Empty = none.
    pub default_extension: String,
}

impl FileDialogState {
    /// Returns the starting directory for file dialogs.
    ///
    /// Uses `last_used_folder` when remembering is enabled, falls back to
    /// `default_work_folder`, then the user's home directory.
    pub fn resolve_directory(&self) -> Option<PathBuf> {
        if self.remember_last_folder {
            if let Some(ref folder) = self.last_used_folder {
                if folder.is_dir() {
                    return Some(folder.clone());
                }
            }
        }
        if !self.default_work_folder.is_empty() {
            let p = PathBuf::from(&self.default_work_folder);
            if p.is_dir() {
                return Some(p);
            }
        }
        dirs::home_dir()
    }

    /// Updates `last_used_folder` from a file path's parent directory.
    pub fn update_last_folder(&mut self, file_path: &Path) {
        if self.remember_last_folder {
            if let Some(parent) = file_path.parent() {
                self.last_used_folder = Some(parent.to_path_buf());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_state() -> FileDialogState {
        FileDialogState {
            remember_last_folder: true,
            default_work_folder: String::new(),
            last_used_folder: None,
            default_extension: String::new(),
        }
    }

    // ── resolve_directory() ─────────────────────────────────────────

    #[test]
    fn test_resolve_returns_last_used_folder() {
        let dir = tempfile::tempdir().unwrap();
        let state = FileDialogState {
            remember_last_folder: true,
            last_used_folder: Some(dir.path().to_path_buf()),
            ..default_state()
        };
        assert_eq!(state.resolve_directory(), Some(dir.path().to_path_buf()));
    }

    #[test]
    fn test_resolve_skips_last_used_when_disabled() {
        let dir = tempfile::tempdir().unwrap();
        let state = FileDialogState {
            remember_last_folder: false,
            last_used_folder: Some(dir.path().to_path_buf()),
            ..default_state()
        };
        // Should NOT return last_used_folder when remember is disabled
        let resolved = state.resolve_directory();
        assert_ne!(resolved, Some(dir.path().to_path_buf()));
    }

    #[test]
    fn test_resolve_skips_nonexistent_last_used() {
        let state = FileDialogState {
            remember_last_folder: true,
            last_used_folder: Some(PathBuf::from("/nonexistent/directory")),
            ..default_state()
        };
        // Falls through because the directory doesn't exist
        let resolved = state.resolve_directory();
        assert_ne!(resolved, Some(PathBuf::from("/nonexistent/directory")));
    }

    #[test]
    fn test_resolve_falls_back_to_default_work_folder() {
        let dir = tempfile::tempdir().unwrap();
        let state = FileDialogState {
            remember_last_folder: true,
            last_used_folder: None,
            default_work_folder: dir.path().to_string_lossy().into_owned(),
            ..default_state()
        };
        assert_eq!(state.resolve_directory(), Some(dir.path().to_path_buf()));
    }

    #[test]
    fn test_resolve_skips_nonexistent_default_work_folder() {
        let state = FileDialogState {
            default_work_folder: "/nonexistent/work/folder".to_string(),
            ..default_state()
        };
        let resolved = state.resolve_directory();
        assert_ne!(resolved, Some(PathBuf::from("/nonexistent/work/folder")));
    }

    #[test]
    fn test_resolve_falls_back_to_home() {
        let state = default_state();
        // No last_used, no default_work_folder — should return home dir
        let resolved = state.resolve_directory();
        assert_eq!(resolved, dirs::home_dir());
    }

    #[test]
    fn test_resolve_priority_last_used_over_default() {
        let dir1 = tempfile::tempdir().unwrap();
        let dir2 = tempfile::tempdir().unwrap();
        let state = FileDialogState {
            remember_last_folder: true,
            last_used_folder: Some(dir1.path().to_path_buf()),
            default_work_folder: dir2.path().to_string_lossy().into_owned(),
            ..default_state()
        };
        // last_used_folder takes priority over default_work_folder
        assert_eq!(state.resolve_directory(), Some(dir1.path().to_path_buf()));
    }

    // ── update_last_folder() ────────────────────────────────────────

    #[test]
    fn test_update_last_folder_sets_parent() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("test.txt");
        let mut state = default_state();
        state.update_last_folder(&file);
        assert_eq!(state.last_used_folder, Some(dir.path().to_path_buf()));
    }

    #[test]
    fn test_update_last_folder_noop_when_disabled() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("test.txt");
        let mut state = FileDialogState {
            remember_last_folder: false,
            ..default_state()
        };
        state.update_last_folder(&file);
        assert!(state.last_used_folder.is_none());
    }

    #[test]
    fn test_update_last_folder_overwrites_previous() {
        let dir1 = tempfile::tempdir().unwrap();
        let dir2 = tempfile::tempdir().unwrap();
        let mut state = default_state();
        state.update_last_folder(&dir1.path().join("a.txt"));
        assert_eq!(state.last_used_folder, Some(dir1.path().to_path_buf()));
        state.update_last_folder(&dir2.path().join("b.txt"));
        assert_eq!(state.last_used_folder, Some(dir2.path().to_path_buf()));
    }
}
