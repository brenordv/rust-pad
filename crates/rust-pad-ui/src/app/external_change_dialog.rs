//! Dialog for prompting the user when an open file has been modified externally.

use super::App;

impl App {
    /// Shows a modal dialog when any document has `external_change_detected = true`.
    ///
    /// Processes one document at a time. On "Reload", reloads from disk.
    /// On "Keep My Version", marks the document as modified and updates
    /// the mtime baseline so the prompt doesn't recur.
    pub(crate) fn show_external_change_dialog(&mut self, ctx: &egui::Context) {
        let pending_idx = self
            .tabs
            .documents
            .iter()
            .position(|d| d.external_change_detected);
        let Some(idx) = pending_idx else { return };

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
            let doc = &mut self.tabs.documents[idx];
            if let Err(e) = doc.reload_from_disk(self.max_file_size_bytes) {
                let msg = format!("Reload failed for '{}': {e:#}", doc.title);
                tracing::warn!("{msg}");
                crate::problem_log::log_problem(&msg);
            }
            doc.external_change_detected = false;
        } else if keep {
            let doc = &mut self.tabs.documents[idx];
            doc.modified = true;
            // Update mtime baseline so we don't re-prompt for this change.
            if let Some(path) = &doc.file_path {
                doc.last_known_mtime = std::fs::metadata(path).and_then(|m| m.modified()).ok();
            }
            doc.external_change_detected = false;
        }
    }
}
