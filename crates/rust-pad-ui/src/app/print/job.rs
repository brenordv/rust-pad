//! Background worker for PDF generation.
//!
//! PDF rendering is CPU-bound and can take hundreds of milliseconds for
//! large documents. Running it on the UI thread would stall egui frames,
//! so we spawn a dedicated one-shot worker thread per request.
//!
//! The worker is distinct from the file-system [`IoWorker`](crate::io_worker::IoWorker):
//! keeping the two separate prevents PDF jobs from starving file reads
//! and vice versa.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::thread;

use anyhow::{anyhow, Context, Result};

use super::pdf;

/// Owned snapshot of the inputs needed to render a PDF. Built on the UI
/// thread and sent across the thread boundary so the worker never touches
/// any `!Send` state on the [`Document`](rust_pad_core::document::Document).
#[derive(Debug, Clone)]
pub struct PrintJobSnapshot {
    /// Full document text, LF-terminated internally.
    pub text: String,
    /// Display title for the header (filename or "Untitled").
    pub title: String,
    /// Full path shown in the header subtitle. Empty for unsaved tabs.
    pub path_display: String,
    /// Local timestamp used in the header.
    pub generated_at: chrono::DateTime<chrono::Local>,
    /// Whether to render line numbers in a left gutter.
    pub show_line_numbers: bool,
}

/// A request dispatched to the worker thread.
pub enum PrintRequest {
    /// Render to a temp file and open it in the default PDF viewer.
    PrintToViewer(PrintJobSnapshot),
    /// Render and write to the given user-chosen path. Does not open.
    ExportToPath {
        snapshot: PrintJobSnapshot,
        target: PathBuf,
    },
}

/// A response returned from the worker thread after a request completes.
#[derive(Debug)]
pub enum PrintResponse {
    /// `PrintToViewer` succeeded; the PDF is at `temp_path` and the
    /// viewer was successfully asked to open it.
    Opened { temp_path: PathBuf },
    /// `ExportToPath` succeeded; the PDF is at `path`.
    Exported { path: PathBuf },
    /// Something went wrong. `temp_path` is set when the PDF was written
    /// successfully but the viewer could not be launched — the UI layer
    /// can then offer "Reveal in file manager" as a fallback.
    Failed {
        message: String,
        temp_path: Option<PathBuf>,
    },
}

/// Worker that owns a response channel and spawns one thread per request.
///
/// The worker is not a long-running thread pool: each `send()` spawns a
/// fresh `std::thread::spawn` so that (a) we only pay for a thread when a
/// request is in flight and (b) a runaway render cannot block subsequent
/// requests. The UI layer enforces single-job-at-a-time with a boolean
/// flag on `App`, so this lightweight model is sufficient.
pub struct PrintWorker {
    tx: Sender<PrintResponse>,
    rx: Receiver<PrintResponse>,
}

impl PrintWorker {
    pub fn new() -> Self {
        let (tx, rx) = channel();
        Self { tx, rx }
    }

    /// Dispatches `request` to a new worker thread.
    pub fn send(&self, request: PrintRequest) {
        let tx = self.tx.clone();
        thread::spawn(move || {
            let response = handle_request(request);
            // If the receiver is gone (app exit) this just drops the response.
            let _ = tx.send(response);
        });
    }

    /// Non-blocking poll for a completed response. Call once per frame.
    pub fn try_recv(&self) -> Option<PrintResponse> {
        self.rx.try_recv().ok()
    }
}

impl Default for PrintWorker {
    fn default() -> Self {
        Self::new()
    }
}

/// Runs a single request on the calling (worker) thread.
fn handle_request(request: PrintRequest) -> PrintResponse {
    match request {
        PrintRequest::PrintToViewer(snapshot) => match render_to_temp(&snapshot) {
            Ok(temp_path) => match opener::open(&temp_path) {
                Ok(()) => PrintResponse::Opened { temp_path },
                Err(e) => PrintResponse::Failed {
                    message: format!(
                        "PDF was generated at {} but could not be opened: {e}",
                        temp_path.display()
                    ),
                    temp_path: Some(temp_path),
                },
            },
            Err(e) => PrintResponse::Failed {
                message: format!("{e:#}"),
                temp_path: None,
            },
        },
        PrintRequest::ExportToPath { snapshot, target } => {
            match render_and_write(&snapshot, &target) {
                Ok(()) => PrintResponse::Exported { path: target },
                Err(e) => PrintResponse::Failed {
                    message: format!("{e:#}"),
                    temp_path: None,
                },
            }
        }
    }
}

/// Renders the snapshot into the OS temp directory and returns the path.
/// The filename uses a `rust-pad-print-` prefix so the startup cleanup in
/// [`crate::app::print::app`] can find and reap stale files.
fn render_to_temp(snapshot: &PrintJobSnapshot) -> Result<PathBuf> {
    let bytes = render(snapshot)?;
    let dir = std::env::temp_dir();
    let file_name = format!("rust-pad-print-{}.pdf", uuid::Uuid::new_v4());
    let path = dir.join(file_name);
    std::fs::write(&path, &bytes)
        .with_context(|| format!("failed to write temp PDF to {}", path.display()))?;
    Ok(path)
}

/// Renders the snapshot into `target`, creating parent directories if
/// needed (matching the behavior of the existing "Save As" path).
fn render_and_write(snapshot: &PrintJobSnapshot, target: &Path) -> Result<()> {
    let bytes = render(snapshot)?;
    if let Some(parent) = target.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).with_context(|| {
                format!("failed to create parent directory {}", parent.display())
            })?;
        }
    }
    std::fs::write(target, &bytes)
        .with_context(|| format!("failed to write PDF to {}", target.display()))?;
    Ok(())
}

fn render(snapshot: &PrintJobSnapshot) -> Result<Vec<u8>> {
    // Catch panics from the PDF library so a runaway unwrap() inside
    // printpdf (historically a weak spot of the crate) cannot kill the
    // worker thread's channel without surfacing a user-visible error.
    let bytes = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        pdf::render_document(
            &snapshot.text,
            &snapshot.title,
            &snapshot.path_display,
            snapshot.generated_at,
            snapshot.show_line_numbers,
        )
    }))
    .map_err(|_| anyhow!("PDF renderer panicked"))??;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn poll_timeout(worker: &PrintWorker, timeout: Duration) -> Option<PrintResponse> {
        let start = Instant::now();
        while start.elapsed() < timeout {
            if let Some(r) = worker.try_recv() {
                return Some(r);
            }
            thread::sleep(Duration::from_millis(10));
        }
        None
    }

    fn sample_snapshot() -> PrintJobSnapshot {
        PrintJobSnapshot {
            text: "hello world\nsecond line\n".to_string(),
            title: "sample.txt".to_string(),
            path_display: String::new(),
            generated_at: chrono::Local::now(),
            show_line_numbers: true,
        }
    }

    #[test]
    fn export_to_path_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("out.pdf");

        let worker = PrintWorker::new();
        worker.send(PrintRequest::ExportToPath {
            snapshot: sample_snapshot(),
            target: target.clone(),
        });

        let resp =
            poll_timeout(&worker, Duration::from_secs(30)).expect("worker did not respond in time");
        match resp {
            PrintResponse::Exported { path } => {
                assert_eq!(path, target);
                let bytes = std::fs::read(&path).unwrap();
                assert!(bytes.starts_with(b"%PDF-"));
            }
            other => panic!("expected Exported, got {other:?}"),
        }
    }

    #[test]
    fn export_to_invalid_path_reports_failure() {
        // Point the "target file" at an existing directory. `std::fs::write`
        // cannot write a regular file over a directory on any platform,
        // so this guarantees a deterministic failure.
        let dir = tempfile::tempdir().unwrap();
        let bad = dir.path().to_path_buf();

        let worker = PrintWorker::new();
        worker.send(PrintRequest::ExportToPath {
            snapshot: sample_snapshot(),
            target: bad,
        });
        let resp =
            poll_timeout(&worker, Duration::from_secs(30)).expect("worker did not respond in time");
        match resp {
            PrintResponse::Failed { message, .. } => {
                assert!(
                    !message.is_empty(),
                    "Failed response should include an error message"
                );
            }
            other => panic!("expected Failed, got {other:?}"),
        }
    }

    #[test]
    fn render_in_worker_produces_valid_pdf() {
        let bytes = render(&sample_snapshot()).unwrap();
        assert!(bytes.starts_with(b"%PDF-"));
    }
}
