//! Background I/O worker for non-blocking file operations.
//!
//! Moves file dialogs and file reads/writes off the UI thread to keep the
//! editor responsive, especially on network drives or with large files.
//!
//! Each request is processed on its own spawned thread, so a blocking file
//! dialog does not prevent concurrent save operations from completing.

use std::path::PathBuf;
use std::sync::mpsc;

/// A request to perform file I/O on a background thread.
#[derive(Debug)]
pub enum IoRequest {
    /// Show an open-file dialog, then read the selected file.
    OpenDialog { start_dir: Option<PathBuf> },
    /// Read a file from a known path (e.g., recent files, session restore).
    ReadFile { path: PathBuf },
    /// Show a save-as dialog, then write content to the chosen path.
    SaveAsDialog {
        content: Vec<u8>,
        suggested_name: String,
        start_dir: Option<PathBuf>,
    },
    /// Write encoded content to a known path.
    SaveFile { path: PathBuf, content: Vec<u8> },
}

/// A response from a completed background I/O operation.
#[derive(Debug)]
pub enum IoResponse {
    /// A file was selected via dialog and read successfully.
    DialogFileOpened { path: PathBuf, bytes: Vec<u8> },
    /// A file was read from a known path.
    FileRead { path: PathBuf, bytes: Vec<u8> },
    /// A save-as dialog completed and the file was written.
    DialogFileSavedAs { path: PathBuf },
    /// A file was saved to a known path.
    FileSaved { path: PathBuf },
    /// A file dialog was cancelled by the user.
    DialogCancelled,
    /// An I/O error occurred.
    Error {
        /// The file path involved, if any.
        path: Option<PathBuf>,
        /// Human-readable error description.
        message: String,
    },
}

/// Context for a pending save-as operation, stored on the UI side.
#[derive(Debug, Clone)]
pub struct SaveAsContext {
    /// Content version at the time the save was initiated.
    pub content_version: u64,
    /// Session ID of the tab being saved (for untitled tabs).
    pub session_id: Option<String>,
    /// Original file path of the tab (for file-backed tabs doing "Save As").
    pub original_path: Option<PathBuf>,
}

/// A pending save-to-known-path operation, stored on the UI side.
#[derive(Debug, Clone)]
pub struct PendingSave {
    /// Path being written to.
    pub path: PathBuf,
    /// Content version at the time the save was initiated.
    pub content_version: u64,
}

/// Tracks active background I/O operations for UI state management.
#[derive(Debug, Clone, Default)]
pub struct IoActivity {
    /// A file dialog is currently showing (prevents opening another).
    pub dialog_open: bool,
    /// Context for a pending save-as dialog.
    pub save_as_context: Option<SaveAsContext>,
    /// In-flight save operations to known paths.
    pub pending_saves: Vec<PendingSave>,
    /// Number of in-flight file reads (not via dialog).
    pub pending_reads: usize,
}

impl IoActivity {
    /// Returns true when any I/O operation is active.
    pub fn is_busy(&self) -> bool {
        self.dialog_open || !self.pending_saves.is_empty() || self.pending_reads > 0
    }

    /// Returns a status message for the status bar, or `None` if idle.
    pub fn status_message(&self) -> Option<&str> {
        if self.dialog_open {
            if self.save_as_context.is_some() {
                Some("Saving...")
            } else {
                Some("Opening...")
            }
        } else if !self.pending_saves.is_empty() {
            Some("Saving...")
        } else if self.pending_reads > 0 {
            Some("Reading...")
        } else {
            None
        }
    }
}

/// Manages background I/O operations using per-request threads and a shared
/// response channel.
///
/// Each call to [`send`](IoWorker::send) spawns a new thread that processes
/// the request and sends the result back through the channel. The UI thread
/// calls [`poll`](IoWorker::poll) each frame to check for completed responses.
pub struct IoWorker {
    tx: mpsc::Sender<IoResponse>,
    rx: mpsc::Receiver<IoResponse>,
}

impl Default for IoWorker {
    fn default() -> Self {
        Self::new()
    }
}

impl IoWorker {
    /// Creates a new I/O worker with an empty response channel.
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        Self { tx, rx }
    }

    /// Sends a request to be processed on a new background thread.
    ///
    /// Returns immediately; the response will arrive via [`poll`](IoWorker::poll).
    pub fn send(&self, request: IoRequest) {
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let response =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| process(request)))
                    .unwrap_or_else(|_| IoResponse::Error {
                        path: None,
                        message: "I/O operation panicked unexpectedly".to_string(),
                    });
            let _ = tx.send(response);
        });
    }

    /// Polls for completed responses (non-blocking).
    ///
    /// Call this in the UI update loop. Returns `None` when no responses are
    /// ready. Call repeatedly until `None` to drain all pending responses.
    pub fn poll(&self) -> Option<IoResponse> {
        self.rx.try_recv().ok()
    }
}

/// Processes a single I/O request on the calling thread.
fn process(request: IoRequest) -> IoResponse {
    match request {
        IoRequest::OpenDialog { start_dir } => {
            let mut dialog = rfd::FileDialog::new().set_title("Open File");
            if let Some(dir) = start_dir {
                dialog = dialog.set_directory(dir);
            }
            match dialog.pick_file() {
                Some(path) => match std::fs::read(&path) {
                    Ok(bytes) => IoResponse::DialogFileOpened { path, bytes },
                    Err(e) => IoResponse::Error {
                        message: format!("Failed to read '{}': {e}", path.display()),
                        path: Some(path),
                    },
                },
                None => IoResponse::DialogCancelled,
            }
        }
        IoRequest::ReadFile { path } => match std::fs::read(&path) {
            Ok(bytes) => IoResponse::FileRead { path, bytes },
            Err(e) => IoResponse::Error {
                message: format!("Failed to read '{}': {e}", path.display()),
                path: Some(path),
            },
        },
        IoRequest::SaveAsDialog {
            content,
            suggested_name,
            start_dir,
        } => {
            let mut dialog = rfd::FileDialog::new()
                .set_title("Save As")
                .set_file_name(&suggested_name);
            if let Some(dir) = start_dir {
                dialog = dialog.set_directory(dir);
            }
            match dialog.save_file() {
                Some(path) => match std::fs::write(&path, &content) {
                    Ok(()) => IoResponse::DialogFileSavedAs { path },
                    Err(e) => IoResponse::Error {
                        message: format!("Failed to write '{}': {e}", path.display()),
                        path: Some(path),
                    },
                },
                None => IoResponse::DialogCancelled,
            }
        }
        IoRequest::SaveFile { path, content } => match std::fs::write(&path, &content) {
            Ok(()) => IoResponse::FileSaved { path },
            Err(e) => IoResponse::Error {
                message: format!("Failed to write '{}': {e}", path.display()),
                path: Some(path),
            },
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// Test helper: blocking poll with timeout.
    fn poll_blocking(worker: &IoWorker, timeout: Duration) -> Option<IoResponse> {
        let start = std::time::Instant::now();
        loop {
            if let Some(r) = worker.poll() {
                return Some(r);
            }
            if start.elapsed() >= timeout {
                return None;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    const TEST_TIMEOUT: Duration = Duration::from_secs(5);

    // ── IoWorker basics ────────────────────────────────────────────────

    #[test]
    fn test_new_worker_has_no_responses() {
        let worker = IoWorker::new();
        assert!(worker.poll().is_none());
    }

    // ── IoActivity ─────────────────────────────────────────────────────

    #[test]
    fn test_default_is_idle() {
        let activity = IoActivity::default();
        assert!(!activity.is_busy());
        assert!(activity.status_message().is_none());
    }

    #[test]
    fn test_dialog_open_shows_opening() {
        let activity = IoActivity {
            dialog_open: true,
            ..Default::default()
        };
        assert!(activity.is_busy());
        assert_eq!(activity.status_message(), Some("Opening..."));
    }

    #[test]
    fn test_save_as_dialog_shows_saving() {
        let activity = IoActivity {
            dialog_open: true,
            save_as_context: Some(SaveAsContext {
                content_version: 1,
                session_id: None,
                original_path: None,
            }),
            ..Default::default()
        };
        assert!(activity.is_busy());
        assert_eq!(activity.status_message(), Some("Saving..."));
    }

    #[test]
    fn test_pending_saves_shows_saving() {
        let activity = IoActivity {
            pending_saves: vec![PendingSave {
                path: PathBuf::from("/tmp/test.txt"),
                content_version: 5,
            }],
            ..Default::default()
        };
        assert!(activity.is_busy());
        assert_eq!(activity.status_message(), Some("Saving..."));
    }

    #[test]
    fn test_pending_reads_shows_reading() {
        let activity = IoActivity {
            pending_reads: 1,
            ..Default::default()
        };
        assert!(activity.is_busy());
        assert_eq!(activity.status_message(), Some("Reading..."));
    }

    // ── ReadFile ───────────────────────────────────────────────────────

    #[test]
    fn test_read_file_success() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.txt");
        std::fs::write(&path, "hello world").unwrap();

        let worker = IoWorker::new();
        worker.send(IoRequest::ReadFile { path: path.clone() });

        match poll_blocking(&worker, TEST_TIMEOUT) {
            Some(IoResponse::FileRead { path: p, bytes: b }) => {
                assert_eq!(p, path);
                assert_eq!(b, b"hello world");
            }
            other => panic!("Expected FileRead, got {other:?}"),
        }
    }

    #[test]
    fn test_read_file_not_found() {
        let worker = IoWorker::new();
        worker.send(IoRequest::ReadFile {
            path: PathBuf::from("/nonexistent/file.txt"),
        });

        match poll_blocking(&worker, TEST_TIMEOUT) {
            Some(IoResponse::Error { path, message }) => {
                assert!(path.is_some());
                assert!(message.contains("Failed to read"));
            }
            other => panic!("Expected Error, got {other:?}"),
        }
    }

    // ── SaveFile ───────────────────────────────────────────────────────

    #[test]
    fn test_save_file_success() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("output.txt");

        let worker = IoWorker::new();
        worker.send(IoRequest::SaveFile {
            path: path.clone(),
            content: b"saved content".to_vec(),
        });

        match poll_blocking(&worker, TEST_TIMEOUT) {
            Some(IoResponse::FileSaved { path: p }) => {
                assert_eq!(p, path);
                assert_eq!(std::fs::read_to_string(&path).unwrap(), "saved content");
            }
            other => panic!("Expected FileSaved, got {other:?}"),
        }
    }

    #[test]
    fn test_save_file_bad_path() {
        let worker = IoWorker::new();
        worker.send(IoRequest::SaveFile {
            path: PathBuf::from("/nonexistent/dir/file.txt"),
            content: b"data".to_vec(),
        });

        match poll_blocking(&worker, TEST_TIMEOUT) {
            Some(IoResponse::Error { path, message }) => {
                assert!(path.is_some());
                assert!(message.contains("Failed to write"));
            }
            other => panic!("Expected Error, got {other:?}"),
        }
    }

    // ── Concurrent requests ────────────────────────────────────────────

    #[test]
    fn test_multiple_concurrent_reads() {
        let dir = tempfile::tempdir().unwrap();
        let path1 = dir.path().join("file1.txt");
        let path2 = dir.path().join("file2.txt");
        std::fs::write(&path1, "content1").unwrap();
        std::fs::write(&path2, "content2").unwrap();

        let worker = IoWorker::new();
        worker.send(IoRequest::ReadFile {
            path: path1.clone(),
        });
        worker.send(IoRequest::ReadFile {
            path: path2.clone(),
        });

        let mut responses = Vec::new();
        let start = std::time::Instant::now();
        while responses.len() < 2 && start.elapsed() < TEST_TIMEOUT {
            if let Some(r) = worker.poll() {
                responses.push(r);
            } else {
                std::thread::sleep(Duration::from_millis(5));
            }
        }

        assert_eq!(responses.len(), 2);
        for r in &responses {
            match r {
                IoResponse::FileRead { path, bytes } => {
                    if *path == path1 {
                        assert_eq!(bytes, b"content1");
                    } else {
                        assert_eq!(*path, path2);
                        assert_eq!(bytes, b"content2");
                    }
                }
                other => panic!("Expected FileRead, got {other:?}"),
            }
        }
    }

    #[test]
    fn test_concurrent_read_and_save() {
        let dir = tempfile::tempdir().unwrap();
        let read_path = dir.path().join("read.txt");
        let save_path = dir.path().join("save.txt");
        std::fs::write(&read_path, "read data").unwrap();

        let worker = IoWorker::new();
        worker.send(IoRequest::ReadFile {
            path: read_path.clone(),
        });
        worker.send(IoRequest::SaveFile {
            path: save_path.clone(),
            content: b"save data".to_vec(),
        });

        let mut got_read = false;
        let mut got_save = false;
        let start = std::time::Instant::now();
        while !(got_read && got_save) && start.elapsed() < TEST_TIMEOUT {
            if let Some(r) = worker.poll() {
                match r {
                    IoResponse::FileRead { .. } => got_read = true,
                    IoResponse::FileSaved { .. } => got_save = true,
                    other => panic!("Unexpected response: {other:?}"),
                }
            } else {
                std::thread::sleep(Duration::from_millis(5));
            }
        }

        assert!(got_read, "Did not receive FileRead response");
        assert!(got_save, "Did not receive FileSaved response");
        assert_eq!(std::fs::read_to_string(&save_path).unwrap(), "save data");
    }
}
