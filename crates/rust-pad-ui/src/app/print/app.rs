//! `impl App` methods that tie the print pipeline into the UI.
//!
//! Exposes `request_print`, `request_export_pdf`, `handle_print_responses`,
//! and a startup cleanup helper for stale temp files.

use std::path::PathBuf;

use super::job::{PrintJobSnapshot, PrintRequest, PrintResponse};
use crate::app::{App, DialogState};

/// Filename prefix used for PDFs written into the OS temp directory by
/// `Print...`. Exposed so the startup cleanup routine can match the same
/// prefix.
pub(crate) const TEMP_PRINT_PREFIX: &str = "rust-pad-print-";

impl App {
    /// "Print..." — builds a snapshot of the active document and dispatches
    /// a PDF-to-temp-plus-viewer job to the background worker.
    pub(crate) fn request_print(&mut self) {
        if self.print_in_progress {
            return;
        }
        let Some(snapshot) = self.build_print_snapshot() else {
            return;
        };
        self.print_in_progress = true;
        self.print_worker
            .send(PrintRequest::PrintToViewer(snapshot));
    }

    /// "Export as PDF..." — shows a synchronous save dialog on the UI
    /// thread, then dispatches a render-and-write job to the background
    /// worker. The dialog itself is quick; the CPU-bound generation is
    /// what we care about keeping off the UI thread.
    pub(crate) fn request_export_pdf(&mut self) {
        if self.print_in_progress {
            return;
        }
        let Some(snapshot) = self.build_print_snapshot() else {
            return;
        };

        let suggested = {
            let doc = self.tabs.active_doc();
            let base = doc
                .file_path
                .as_ref()
                .and_then(|p| p.file_stem().map(|s| s.to_string_lossy().into_owned()))
                .unwrap_or_else(|| {
                    // Fall back to the tab title minus any trailing extension.
                    let t = doc.title.clone();
                    match t.rsplit_once('.') {
                        Some((stem, _)) if !stem.is_empty() => stem.to_string(),
                        _ => t,
                    }
                });
            format!("{base}.pdf")
        };
        let start_dir = self.file_dialog.resolve_directory();

        let mut dialog = rfd::FileDialog::new()
            .set_title("Export as PDF")
            .set_file_name(&suggested)
            .add_filter("PDF", &["pdf"]);
        if let Some(dir) = start_dir {
            dialog = dialog.set_directory(dir);
        }
        let dialog_result = dialog.save_file();

        let Some(mut target) = dialog_result else {
            // User cancelled — nothing to do.
            return;
        };
        if target.extension().is_none() {
            target.set_extension("pdf");
        }
        self.file_dialog.update_last_folder(&target);

        self.print_in_progress = true;
        self.print_worker
            .send(PrintRequest::ExportToPath { snapshot, target });
    }

    /// Polls the print worker once per frame and updates UI state.
    pub(crate) fn handle_print_responses(&mut self) {
        while let Some(response) = self.print_worker.try_recv() {
            self.print_in_progress = false;
            match response {
                PrintResponse::Opened { temp_path } => {
                    tracing::info!("Print: opened {} in default viewer", temp_path.display());
                    self.print_last_status = Some(format!(
                        "Opened in default PDF viewer: {}",
                        temp_path.display()
                    ));
                }
                PrintResponse::Exported { path } => {
                    tracing::info!("Export as PDF: wrote {}", path.display());
                    self.print_last_status = Some(format!("Exported to {}", path.display()));
                }
                PrintResponse::Failed { message, temp_path } => {
                    tracing::error!("Print/export failed: {message}");
                    self.print_last_status = None;
                    self.dialog_state = DialogState::PrintError { message, temp_path };
                }
            }
        }
    }

    /// Builds an owned snapshot of the active document's state suitable
    /// for sending across the worker thread boundary. Returns `None` if
    /// the active buffer is empty (matching the disabled-menu-entry rule).
    fn build_print_snapshot(&self) -> Option<PrintJobSnapshot> {
        let doc = self.tabs.active_doc();
        if doc.buffer.is_empty() {
            return None;
        }
        let text = doc.buffer.to_string();
        let (title, path_display) = if let Some(path) = &doc.file_path {
            let title = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| doc.title.clone());
            let path_display = path.to_string_lossy().into_owned();
            (title, path_display)
        } else {
            (doc.title.clone(), String::new())
        };
        Some(PrintJobSnapshot {
            text,
            title,
            path_display,
            generated_at: chrono::Local::now(),
            show_line_numbers: self.print_show_line_numbers,
        })
    }

    /// Returns `true` when the active document has content that can be
    /// printed. Used to gate menu entries.
    pub(crate) fn can_print_active(&self) -> bool {
        !self.print_in_progress && !self.tabs.active_doc().buffer.is_empty()
    }

    /// Best-effort cleanup of stale temp PDFs produced by previous
    /// sessions. Runs once at startup. Failures are swallowed — a leaked
    /// temp file is not worth surfacing to the user.
    pub(crate) fn cleanup_stale_print_temp_files() {
        let Ok(entries) = std::fs::read_dir(std::env::temp_dir()) else {
            return;
        };
        let cutoff =
            std::time::SystemTime::now().checked_sub(std::time::Duration::from_secs(24 * 60 * 60));
        let Some(cutoff) = cutoff else {
            return;
        };

        let mut removed = 0usize;
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            if !name.starts_with(TEMP_PRINT_PREFIX) || !name.ends_with(".pdf") {
                continue;
            }
            if let Ok(meta) = entry.metadata() {
                if let Ok(modified) = meta.modified() {
                    if modified < cutoff && std::fs::remove_file(&path).is_ok() {
                        removed += 1;
                    }
                }
            }
        }
        if removed > 0 {
            tracing::debug!("Removed {removed} stale rust-pad-print-*.pdf temp files");
        }
    }

    /// Renders the "Print Error" dialog when a print/export job failed.
    /// Offers OK plus, when available, a "Reveal in file manager" button
    /// so the user can still access the generated PDF.
    pub(crate) fn show_print_error_dialog(&mut self, ctx: &eframe::egui::Context) {
        let DialogState::PrintError { message, temp_path } = &self.dialog_state else {
            return;
        };
        let message = message.clone();
        let temp_path: Option<PathBuf> = temp_path.clone();

        let mut open = true;
        eframe::egui::Window::new("Print / Export Failed")
            .collapsible(false)
            .resizable(false)
            .anchor(eframe::egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .open(&mut open)
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing.y = 8.0;
                ui.label(&message);
                ui.add_space(4.0);

                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    if let Some(path) = &temp_path {
                        if ui.button("  Reveal in File Manager  ").clicked() {
                            // Best-effort reveal; swallow any error.
                            let _ = opener::reveal(path);
                            self.dialog_state = DialogState::None;
                        }
                    }
                    if ui.button("  OK  ").clicked() {
                        self.dialog_state = DialogState::None;
                    }
                });
            });
        if !open {
            self.dialog_state = DialogState::None;
        }
    }
}
