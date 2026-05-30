//! Dialog for prompting the user when an open file has been modified externally.

use super::App;

impl App {
    /// Returns the index of the active document iff it has a pending
    /// external change. Inactive tabs with the flag set are deliberately
    /// ignored — the prompt only surfaces when the user is actually looking
    /// at the affected tab. Switching to a flagged tab will surface the
    /// prompt on the next frame.
    pub(crate) fn pending_external_change_idx(&self) -> Option<usize> {
        let active = self.tabs.active;
        let doc = self.tabs.documents.get(active)?;
        if doc.external_change_detected {
            Some(active)
        } else {
            None
        }
    }

    /// Handles the "Reload" action for a document with an external change.
    ///
    /// Reloads the document content from disk and clears the external change flag.
    /// Logs an error if the reload fails.
    pub(crate) fn accept_external_reload(&mut self, idx: usize) {
        let doc = &mut self.tabs.documents[idx];
        if let Err(e) = doc.reload_from_disk(self.max_file_size_bytes) {
            crate::problem_log::warn_problem(&format!("Reload failed for '{}': {e:#}", doc.title));
        }
        doc.external_change_detected = false;
    }

    /// Handles the "Keep My Version" action for a document with an external change.
    ///
    /// Marks the document as modified and updates the mtime baseline so the
    /// prompt doesn't recur.
    pub(crate) fn dismiss_external_change(&mut self, idx: usize) {
        let doc = &mut self.tabs.documents[idx];
        doc.modified = true;
        if let Some(path) = &doc.file_path {
            doc.last_known_mtime = std::fs::metadata(path).and_then(|m| m.modified()).ok();
        }
        doc.external_change_detected = false;
    }

    /// Shows a modal dialog when any document has `external_change_detected = true`.
    ///
    /// Processes one document at a time. On "Reload", reloads from disk.
    /// On "Keep My Version", marks the document as modified and updates
    /// the mtime baseline so the prompt doesn't recur.
    pub(crate) fn show_external_change_dialog(&mut self, ctx: &egui::Context) {
        let Some(idx) = self.pending_external_change_idx() else {
            return;
        };

        let title = self.tabs.documents[idx].title.clone();
        let has_unsaved = self.tabs.documents[idx].modified;
        let mut reload = false;
        let mut keep = false;

        egui::Window::new("File Changed on Disk")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                if has_unsaved {
                    ui.label(format!(
                        "The file '{}' has been modified by another program.\n\n\
                         You also have unsaved changes. Reloading will discard your changes.",
                        title
                    ));
                } else {
                    ui.label(format!(
                        "The file '{}' has been modified by another program.\n\n\
                         Do you want to reload it?",
                        title
                    ));
                }
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Reload").clicked() {
                        reload = true;
                    }
                    if ui.button("Keep My Version").clicked() {
                        keep = true;
                    }
                });
            });

        if reload {
            self.accept_external_reload(idx);
        } else if keep {
            self.dismiss_external_change(idx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::test_app;

    #[test]
    fn pending_external_change_idx_none_when_no_changes() {
        let app = test_app();
        assert_eq!(app.pending_external_change_idx(), None);
    }

    #[test]
    fn pending_external_change_idx_returns_active_when_flagged() {
        let mut app = test_app();
        app.tabs.active_doc_mut().external_change_detected = true;
        assert_eq!(app.pending_external_change_idx(), Some(app.tabs.active));
    }

    #[test]
    fn pending_external_change_idx_none_when_only_inactive_tab_flagged() {
        let mut app = test_app();
        // Open a second tab and switch to it; flag the inactive (first) tab.
        app.new_tab();
        assert_eq!(app.tabs.active, 1);
        app.tabs.documents[0].external_change_detected = true;
        // Active doc is not flagged → no prompt.
        assert_eq!(app.pending_external_change_idx(), None);
        // Switch back to flagged tab → prompt now surfaces.
        app.tabs.switch_to(0);
        assert_eq!(app.pending_external_change_idx(), Some(0));
    }

    #[test]
    fn accept_external_reload_clears_flag() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("test.txt");
        std::fs::write(&file, "original content\n").unwrap();

        let mut app = test_app();
        app.tabs.open_file(&file).unwrap();
        let idx = app.tabs.active;
        app.tabs.documents[idx].external_change_detected = true;

        // Modify file externally
        std::fs::write(&file, "new content\n").unwrap();

        app.accept_external_reload(idx);
        assert!(!app.tabs.documents[idx].external_change_detected);
        assert!(app.tabs.documents[idx]
            .buffer
            .to_string()
            .contains("new content"));
    }

    #[test]
    fn dismiss_external_change_marks_modified_and_clears_flag() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("test.txt");
        std::fs::write(&file, "original\n").unwrap();

        let mut app = test_app();
        app.tabs.open_file(&file).unwrap();
        let idx = app.tabs.active;
        app.tabs.documents[idx].external_change_detected = true;
        app.tabs.documents[idx].modified = false;

        app.dismiss_external_change(idx);
        assert!(!app.tabs.documents[idx].external_change_detected);
        assert!(app.tabs.documents[idx].modified);
        assert!(app.tabs.documents[idx].last_known_mtime.is_some());
    }

    #[test]
    fn dismiss_external_change_updates_mtime_baseline() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("test.txt");
        std::fs::write(&file, "v1\n").unwrap();

        let mut app = test_app();
        app.tabs.open_file(&file).unwrap();
        let idx = app.tabs.active;
        let old_mtime = app.tabs.documents[idx].last_known_mtime;

        // Modify file so mtime changes
        std::thread::sleep(std::time::Duration::from_millis(50));
        std::fs::write(&file, "v2\n").unwrap();

        app.tabs.documents[idx].external_change_detected = true;
        app.dismiss_external_change(idx);

        // Mtime should be updated to the new file's mtime
        let new_mtime = app.tabs.documents[idx].last_known_mtime;
        assert_ne!(old_mtime, new_mtime);
    }
}
