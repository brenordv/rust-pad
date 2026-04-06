//! File I/O operations for the editor application.
//!
//! Handles opening files, saving (including save-as), creating new tabs,
//! session cleanup, and closing tabs with unsaved-change prompts.
//!
//! File dialogs and file reads run on background threads via the
//! [`IoWorker`](crate::io_worker::IoWorker) to keep the UI responsive.
//! Saves to known paths also run in the background. Results are polled
//! each frame in [`App::handle_io_responses`](super::App::handle_io_responses).

use crate::io_worker::{IoRequest, PendingSave, SaveAsContext};
use rust_pad_config::session::generate_session_id;

use super::{App, DialogState};

impl App {
    /// Opens a file dialog on a background thread.
    ///
    /// The dialog and file read happen off the UI thread. The result is
    /// handled in [`handle_io_responses`](App::handle_io_responses).
    pub(crate) fn open_file_dialog(&mut self) {
        if self.io_activity.dialog_open {
            return;
        }

        self.io_activity.dialog_open = true;
        self.io_worker.send(IoRequest::OpenDialog {
            start_dir: self.file_dialog.resolve_directory(),
        });
    }

    /// Opens a file from a known path on a background thread.
    ///
    /// Used by the recent files menu. If the file is already open,
    /// switches to the existing tab immediately without I/O.
    pub(crate) fn open_file_path(&mut self, path: &std::path::Path) {
        // Check for duplicate synchronously to avoid unnecessary reads
        if let Some(idx) = self
            .tabs
            .documents
            .iter()
            .position(|d| d.file_path.as_deref() == Some(path))
        {
            self.tabs.switch_to(idx);
            return;
        }

        self.io_activity.pending_reads += 1;
        self.io_worker.send(IoRequest::ReadFile {
            path: path.to_path_buf(),
        });
    }

    /// Saves the active document.
    ///
    /// For file-backed documents, encodes content on the UI thread and
    /// writes on a background thread. For untitled documents, opens a
    /// save-as dialog.
    pub(crate) fn save_active(&mut self) {
        let doc = self.tabs.active_doc();
        if let Some(path) = doc.file_path.clone() {
            let content = match doc.encode_for_save() {
                Ok(bytes) => bytes,
                Err(e) => {
                    tracing::error!("Failed to encode document: {e:#}");
                    return;
                }
            };
            let version = doc.content_version;

            self.io_activity.pending_saves.push(PendingSave {
                path: path.clone(),
                content_version: version,
            });
            self.io_worker.send(IoRequest::SaveFile { path, content });
        } else {
            self.save_as_dialog();
        }
    }

    /// Opens a save-as dialog on a background thread.
    ///
    /// Encodes the document content on the UI thread, then sends both
    /// the content and dialog parameters to the background thread.
    pub(crate) fn save_as_dialog(&mut self) {
        if self.io_activity.dialog_open {
            return;
        }

        let doc = self.tabs.active_doc();
        let content = match doc.encode_for_save() {
            Ok(bytes) => bytes,
            Err(e) => {
                tracing::error!("Failed to encode document: {e:#}");
                return;
            }
        };

        self.io_activity.dialog_open = true;
        self.io_activity.save_as_context = Some(SaveAsContext {
            content_version: doc.content_version,
            session_id: doc.session_id.clone(),
            original_path: doc.file_path.clone(),
        });

        self.io_worker.send(IoRequest::SaveAsDialog {
            content,
            suggested_name: doc.title.clone(),
            start_dir: self.file_dialog.resolve_directory(),
        });
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
