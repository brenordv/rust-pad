//! File I/O operations for the editor application.
//!
//! Handles opening files, saving (including save-as), creating new tabs,
//! session cleanup, and closing tabs with unsaved-change prompts.

use std::path::Path;

use rust_pad_config::session::generate_session_id;

use super::{App, DialogState};

impl App {
    /// Opens a file dialog and loads the selected file into a new tab.
    pub(crate) fn open_file_dialog(&mut self) {
        let mut dialog = rfd::FileDialog::new().set_title("Open File");
        if let Some(dir) = self.resolve_dialog_directory() {
            dialog = dialog.set_directory(dir);
        }
        if let Some(path) = dialog.pick_file() {
            self.update_last_used_folder(&path);
            if let Err(e) = self.tabs.open_file(&path) {
                tracing::error!("Failed to open file: {e:#}");
            }
        }
    }

    /// Saves the active document, or opens a save-as dialog if it has no file path.
    pub(crate) fn save_active(&mut self) {
        let doc = self.tabs.active_doc_mut();
        if doc.file_path.is_some() {
            if let Err(e) = doc.save() {
                tracing::error!("Failed to save: {e:#}");
            }
        } else {
            self.save_as_dialog();
        }
    }

    /// Opens a save-as dialog and saves the active document to the chosen path.
    pub(crate) fn save_as_dialog(&mut self) {
        let mut dialog = rfd::FileDialog::new()
            .set_title("Save As")
            .set_file_name(&self.tabs.active_doc().title);
        if let Some(dir) = self.resolve_dialog_directory() {
            dialog = dialog.set_directory(dir);
        }
        if let Some(path) = dialog.save_file() {
            self.update_last_used_folder(&path);
            // Clean up session content before saving (transitions unsaved -> file-backed)
            self.cleanup_session_for_tab(self.tabs.active);
            let doc = self.tabs.active_doc_mut();
            if let Err(e) = doc.save_to(&path) {
                tracing::error!("Failed to save: {e:#}");
            } else {
                doc.session_id = None;
            }
        }
    }

    /// Creates a new empty tab and assigns it a session ID.
    pub(crate) fn new_tab(&mut self) {
        self.tabs.new_tab();
        self.tabs.documents.last_mut().unwrap().session_id = Some(generate_session_id());
    }

    /// Cleans up persisted session content for a tab being closed.
    pub(crate) fn cleanup_session_for_tab(&self, idx: usize) {
        if let Some(session_id) = &self.tabs.documents[idx].session_id {
            if let Some(store) = &self.session_store {
                let _ = store.delete_content(session_id);
            }
        }
    }

    /// Requests closing a tab, prompting for unsaved changes if modified.
    pub(crate) fn request_close_tab(&mut self, idx: usize) {
        if idx < self.tabs.tab_count() && self.tabs.documents[idx].modified {
            self.dialog_state = DialogState::ConfirmClose(idx);
        } else if idx < self.tabs.tab_count() {
            self.cleanup_session_for_tab(idx);
            self.tabs.close_tab(idx);
        }
    }

    /// Returns the starting directory for file dialogs.
    ///
    /// Uses `last_used_folder` when remembering is enabled, falls back to
    /// `default_work_folder`, then the user's home directory.
    fn resolve_dialog_directory(&self) -> Option<std::path::PathBuf> {
        if self.remember_last_folder {
            if let Some(ref folder) = self.last_used_folder {
                if folder.is_dir() {
                    return Some(folder.clone());
                }
            }
        }
        if !self.default_work_folder.is_empty() {
            let p = std::path::PathBuf::from(&self.default_work_folder);
            if p.is_dir() {
                return Some(p);
            }
        }
        dirs::home_dir()
    }

    /// Checks all live-monitored documents for external file changes and reloads them.
    pub(crate) fn check_live_monitored_files(&mut self) {
        for doc in &mut self.tabs.documents {
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
    }

    /// Auto-saves all modified file-backed documents.
    pub(crate) fn auto_save_all(&mut self) {
        for doc in &mut self.tabs.documents {
            if doc.modified && doc.file_path.is_some() {
                if let Err(e) = doc.save() {
                    tracing::warn!("Auto-save failed for '{}': {e:#}", doc.title);
                }
            }
        }
    }

    /// Updates `last_used_folder` from a file path's parent directory.
    fn update_last_used_folder(&mut self, file_path: &Path) {
        if self.remember_last_folder {
            if let Some(parent) = file_path.parent() {
                self.last_used_folder = Some(parent.to_path_buf());
            }
        }
    }
}
