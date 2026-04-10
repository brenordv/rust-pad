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
            max_file_size_bytes: self.max_file_size_bytes,
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
            max_file_size_bytes: self.max_file_size_bytes,
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
        self.save_as_dialog_impl(false);
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

    /// Requests a reload of the active document from disk.
    ///
    /// If the document is modified, prompts for confirmation first.
    /// If unmodified (or untitled), reloads immediately.
    pub(crate) fn request_reload_from_disk(&mut self) {
        let doc = self.tabs.active_doc();
        if doc.file_path.is_none() {
            return;
        }
        if doc.modified {
            self.dialog_state = DialogState::ConfirmReload;
        } else {
            self.do_reload_from_disk();
        }
    }

    /// Performs the actual reload from disk on the active document.
    pub(crate) fn do_reload_from_disk(&mut self) {
        let doc = self.tabs.active_doc_mut();
        if let Err(e) = doc.reload_from_disk(self.max_file_size_bytes) {
            tracing::error!("Failed to reload from disk: {e:#}");
        }
    }

    /// Opens a save-a-copy dialog: saves content to a new path without
    /// changing the active document's path, title, or modified state.
    pub(crate) fn save_copy_dialog(&mut self) {
        self.save_as_dialog_impl(true);
    }

    /// Shared implementation for save-as and save-a-copy dialogs.
    ///
    /// When `is_copy` is true, the document state is not updated after
    /// the file is written (title, path, and modified flag remain unchanged).
    fn save_as_dialog_impl(&mut self, is_copy: bool) {
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
            is_copy,
        });

        self.io_worker.send(IoRequest::SaveAsDialog {
            content,
            suggested_name: doc.title.clone(),
            start_dir: self.file_dialog.resolve_directory(),
        });
    }

    /// Closes all tabs that have no unsaved changes.
    ///
    /// Pinned tabs are skipped (when pin support is added).
    /// Iterates in reverse to keep indices stable.
    pub(crate) fn close_unchanged_tabs(&mut self) {
        let mut i = self.tabs.tab_count();
        while i > 0 {
            i -= 1;
            if !self.tabs.documents[i].modified {
                self.cleanup_session_for_tab(i);
                self.tabs.close_tab(i);
            }
        }
    }

    /// Closes all tabs except the active one.
    ///
    /// Modified tabs are closed without prompting (same as existing "Close Others").
    pub(crate) fn close_all_but_active(&mut self) {
        let keep = self.tabs.active;
        let mut i = self.tabs.tab_count();
        while i > 0 {
            i -= 1;
            if i != keep {
                self.cleanup_session_for_tab(i);
                self.tabs.close_tab(i);
            }
        }
        self.tabs.active = 0;
    }

    /// Closes all tabs: first closes unmodified ones silently, then
    /// prompts sequentially for each remaining modified tab.
    pub(crate) fn close_all_tabs(&mut self) {
        // First pass: close all unmodified tabs silently
        let mut i = self.tabs.tab_count();
        while i > 0 {
            i -= 1;
            if !self.tabs.documents[i].modified {
                self.cleanup_session_for_tab(i);
                self.tabs.close_tab(i);
            }
        }

        // If modified tabs remain, enter close-all mode and prompt for the first one
        if self.tabs.tab_count() > 0 && self.tabs.documents[0].modified {
            self.closing_all = true;
            self.tabs.switch_to(0);
            self.dialog_state = DialogState::ConfirmClose(0);
        }
    }

    /// Continues the close-all flow by prompting for the next modified tab.
    ///
    /// Called after a ConfirmClose dialog resolves when `closing_all` is true.
    pub(crate) fn continue_close_all(&mut self) {
        if !self.closing_all {
            return;
        }

        // Find the next modified tab
        if let Some(idx) = self.tabs.documents.iter().position(|d| d.modified) {
            self.tabs.switch_to(idx);
            self.dialog_state = DialogState::ConfirmClose(idx);
        } else {
            // No more modified tabs — close-all is complete
            self.closing_all = false;
        }
    }
}
