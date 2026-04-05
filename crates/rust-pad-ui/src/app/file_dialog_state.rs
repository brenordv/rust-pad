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
