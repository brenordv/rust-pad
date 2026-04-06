//! File I/O operations for the editor application.
//!
//! Handles opening files, saving (including save-as), creating new tabs,
//! session cleanup, and closing tabs with unsaved-change prompts.

use rust_pad_config::session::generate_session_id;

use super::{App, DialogState};

impl App {
    /// Opens a file dialog and loads the selected file into a new tab.
    pub(crate) fn open_file_dialog(&mut self) {
        let mut dialog = rfd::FileDialog::new().set_title("Open File");
        if let Some(dir) = self.file_dialog.resolve_directory() {
            dialog = dialog.set_directory(dir);
        }
        if let Some(path) = dialog.pick_file() {
            self.file_dialog.update_last_folder(&path);
            if let Err(e) = self.tabs.open_file(&path) {
                tracing::error!("Failed to open file: {e:#}");
            } else {
                self.recent_files.track(&path);
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
        if let Some(dir) = self.file_dialog.resolve_directory() {
            dialog = dialog.set_directory(dir);
        }
        if let Some(path) = dialog.save_file() {
            self.file_dialog.update_last_folder(&path);
            // Clean up session content before saving (transitions unsaved -> file-backed)
            self.cleanup_session_for_tab(self.tabs.active);
            let doc = self.tabs.active_doc_mut();
            if let Err(e) = doc.save_to(&path) {
                tracing::error!("Failed to save: {e:#}");
            } else {
                doc.session_id = None;
                self.recent_files.track(&path);
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
}
