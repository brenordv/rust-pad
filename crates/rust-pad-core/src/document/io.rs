//! File I/O operations for documents.
//!
//! Handles opening files from disk (with encoding/line-ending detection),
//! saving documents back to disk, and managing persistent undo history.

use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result};

use crate::buffer::TextBuffer;
use crate::encoding::{
    apply_line_ending, decode_bytes, detect_encoding, detect_line_ending, encode_string,
    normalize_line_endings,
};
use crate::history::{doc_id_for_path, HistoryConfig, PersistenceLayer, UndoManager};
use crate::indent::detect_indent;

use super::{Document, LineChangeState, ScrollbarDrag};

/// Validates that a file's size is within the given limit.
///
/// Returns the file size in bytes on success.
///
/// # Errors
///
/// Returns an error if the file metadata cannot be read or the file
/// exceeds `max_bytes`.
pub fn validate_file_size(path: &Path, max_bytes: u64) -> Result<u64> {
    let metadata = std::fs::metadata(path)
        .with_context(|| format!("failed to read file metadata: {}", path.display()))?;
    let size = metadata.len();
    if size > max_bytes {
        let size_mb = size as f64 / (1024.0 * 1024.0);
        let limit_mb = max_bytes as f64 / (1024.0 * 1024.0);
        anyhow::bail!(
            "File is too large ({size_mb:.1} MB). Maximum allowed size is {limit_mb:.0} MB. \
             Adjust 'max_file_size_mb' in the settings or config file to change this limit."
        );
    }
    Ok(size)
}

impl Document {
    /// Opens a document from a file path with in-memory-only history.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be read or decoded.
    pub fn open(path: &std::path::Path) -> Result<Self> {
        Self::open_internal(path, None)
    }

    /// Opens a document from a file path with persistent history.
    ///
    /// Loads existing undo history from disk if available.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be read/decoded or history loading fails.
    pub fn open_with_persistence(
        path: &std::path::Path,
        persistence: Arc<PersistenceLayer>,
        config: &HistoryConfig,
    ) -> Result<Self> {
        Self::open_internal(path, Some((persistence, config)))
    }

    /// Creates a document from raw file bytes with encoding auto-detection.
    ///
    /// Handles encoding detection, decoding, line-ending normalization, and
    /// indent detection. Used by both synchronous file open and the async
    /// I/O path when file bytes arrive from a background thread.
    ///
    /// # Errors
    ///
    /// Returns an error if decoding fails or history loading fails.
    pub fn from_bytes(
        bytes: &[u8],
        path: &std::path::Path,
        persistence: Option<(Arc<PersistenceLayer>, &HistoryConfig)>,
    ) -> Result<Self> {
        let encoding = detect_encoding(bytes);
        let raw_text = decode_bytes(bytes, encoding)
            .with_context(|| format!("failed to decode file: {}", path.display()))?;

        let line_ending = detect_line_ending(&raw_text);
        let text = normalize_line_endings(&raw_text);
        let indent_style = detect_indent(&text);
        let buffer = TextBuffer::from(text.as_str());
        let line_count = buffer.len_lines().max(1);

        let title = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Untitled".to_string());

        let history = match persistence {
            Some((pl, config)) => {
                let doc_id = doc_id_for_path(path);
                UndoManager::load_or_new(doc_id, config.clone(), Some(pl))
                    .context("failed to load undo history")?
            }
            None => UndoManager::in_memory(),
        };

        Ok(Self {
            buffer,
            cursor: crate::cursor::Cursor::new(),
            secondary_cursors: Vec::new(),
            history,
            file_path: Some(path.to_path_buf()),
            encoding,
            line_ending,
            indent_style,
            modified: false,
            title,
            line_changes: vec![LineChangeState::Unchanged; line_count],
            scroll_y: 0.0,
            scroll_x: 0.0,
            cursor_activity_time: 0.0,
            scrollbar_drag: ScrollbarDrag::None,
            session_id: None,
            scroll_to_cursor: false,
            last_saved_at: None,
            live_monitoring: false,
            last_known_mtime: std::fs::metadata(path).and_then(|m| m.modified()).ok(),
            content_version: 0,
            cached_max_line_chars: None,
            cached_occurrences: None,
            render_cache: None,
        })
    }

    /// Internal open shared by `open()` and `open_with_persistence()`.
    fn open_internal(
        path: &std::path::Path,
        persistence: Option<(Arc<PersistenceLayer>, &HistoryConfig)>,
    ) -> Result<Self> {
        let bytes = std::fs::read(path)
            .with_context(|| format!("failed to read file: {}", path.display()))?;
        Self::from_bytes(&bytes, path, persistence)
    }

    /// Saves the document to its file path.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be written.
    pub fn save(&mut self) -> Result<()> {
        let path = self
            .file_path
            .as_ref()
            .context("no file path set for this document")?
            .clone();
        self.save_to(&path)
    }

    /// Saves the document to a specific path.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be written.
    pub fn save_to(&mut self, path: &std::path::Path) -> Result<()> {
        let text = self.buffer.to_string();
        let with_endings = apply_line_ending(&text, self.line_ending);
        let bytes = encode_string(&with_endings, self.encoding)
            .context("failed to encode document for saving")?;

        std::fs::write(path, &bytes)
            .with_context(|| format!("failed to write file: {}", path.display()))?;

        self.file_path = Some(path.to_path_buf());
        self.title = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Untitled".to_string());
        self.modified = false;
        self.last_saved_at = Some(chrono::Local::now());
        self.last_known_mtime = std::fs::metadata(path).and_then(|m| m.modified()).ok();

        // Mark all modified lines as saved
        for change in &mut self.line_changes {
            if *change == LineChangeState::Modified {
                *change = LineChangeState::Saved;
            }
        }

        Ok(())
    }

    /// Encodes the document content for saving: applies line endings and
    /// encodes to bytes.
    ///
    /// Use this to prepare content for a background save via the I/O worker.
    ///
    /// # Errors
    ///
    /// Returns an error if encoding fails.
    pub fn encode_for_save(&self) -> Result<Vec<u8>> {
        let text = self.buffer.to_string();
        let with_endings = apply_line_ending(&text, self.line_ending);
        encode_string(&with_endings, self.encoding).context("failed to encode document for saving")
    }

    /// Updates document state after a successful background save.
    ///
    /// Only clears the modified flag if the document has not been edited since
    /// the save was initiated (detected via `saved_version` matching the
    /// current `content_version`).
    pub fn mark_saved(&mut self, path: &std::path::Path, saved_version: u64) {
        self.file_path = Some(path.to_path_buf());
        self.title = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Untitled".to_string());
        self.last_saved_at = Some(chrono::Local::now());
        self.last_known_mtime = std::fs::metadata(path).and_then(|m| m.modified()).ok();

        if self.content_version == saved_version {
            self.modified = false;
            for change in &mut self.line_changes {
                if *change == LineChangeState::Modified {
                    *change = LineChangeState::Saved;
                }
            }
        }
    }

    /// Reloads the document from disk, preserving metadata like live_monitoring.
    ///
    /// Used by live file monitoring to pick up external changes.
    /// When `max_file_size_bytes` is `Some`, the file size is validated
    /// before reading to prevent out-of-memory conditions.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be read, decoded, or exceeds
    /// the size limit.
    pub fn reload_from_disk(&mut self, max_file_size_bytes: Option<u64>) -> Result<()> {
        let path = self
            .file_path
            .as_ref()
            .context("no file path set for reload")?
            .clone();

        if let Some(max_bytes) = max_file_size_bytes {
            validate_file_size(&path, max_bytes)?;
        }

        let bytes =
            std::fs::read(&path).with_context(|| format!("failed to read: {}", path.display()))?;
        let encoding = detect_encoding(&bytes);
        let raw_text = decode_bytes(&bytes, encoding)
            .with_context(|| format!("failed to decode: {}", path.display()))?;
        let line_ending = detect_line_ending(&raw_text);
        let text = normalize_line_endings(&raw_text);

        self.buffer = TextBuffer::from(text.as_str());
        self.encoding = encoding;
        self.line_ending = line_ending;
        self.modified = false;
        self.last_known_mtime = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
        self.sync_line_changes();
        self.bump_version();

        Ok(())
    }

    /// Flushes the undo history to disk.
    ///
    /// No-op if using in-memory-only history.
    ///
    /// # Errors
    ///
    /// Returns an error if the disk write fails.
    pub fn flush_history(&mut self) -> Result<()> {
        self.history.flush()
    }

    /// Deletes all persisted undo history for this document.
    ///
    /// Called when a tab is explicitly closed.
    ///
    /// # Errors
    ///
    /// Returns an error if disk cleanup fails.
    pub fn delete_history(&mut self) -> Result<()> {
        self.history.delete_history()
    }

    /// Returns the document's history identifier.
    pub fn doc_id(&self) -> &str {
        self.history.doc_id()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::encoding::{LineEnding, TextEncoding};

    // ── from_bytes ─────────────────────────────────────────────────────

    #[test]
    fn test_from_bytes_utf8() {
        let content = "hello world";
        let bytes = content.as_bytes();
        let path = std::path::Path::new("test.txt");
        let doc = Document::from_bytes(bytes, path, None).unwrap();

        assert_eq!(doc.buffer.to_string(), "hello world");
        assert_eq!(doc.encoding, TextEncoding::Ascii);
        assert_eq!(doc.title, "test.txt");
        assert_eq!(doc.file_path.as_deref(), Some(path));
        assert!(!doc.modified);
    }

    #[test]
    fn test_from_bytes_detects_crlf() {
        let content = "line1\r\nline2\r\n";
        let bytes = content.as_bytes();
        let doc = Document::from_bytes(bytes, std::path::Path::new("f.txt"), None).unwrap();

        assert_eq!(doc.line_ending, LineEnding::CrLf);
        // Internal buffer normalizes to LF
        assert_eq!(doc.buffer.to_string(), "line1\nline2\n");
    }

    #[test]
    fn test_from_bytes_utf8_bom() {
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice("hello".as_bytes());
        let doc = Document::from_bytes(&bytes, std::path::Path::new("bom.txt"), None).unwrap();

        assert_eq!(doc.encoding, TextEncoding::Utf8Bom);
        assert_eq!(doc.buffer.to_string(), "hello");
    }

    #[test]
    fn test_from_bytes_equivalent_to_open() {
        let dir = std::env::temp_dir().join("rust_pad_test_from_bytes_equiv");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("test.txt");
        std::fs::write(&path, "hello\nworld").unwrap();

        let bytes = std::fs::read(&path).unwrap();
        let doc_from_bytes = Document::from_bytes(&bytes, &path, None).unwrap();
        let doc_from_open = Document::open(&path).unwrap();

        assert_eq!(
            doc_from_bytes.buffer.to_string(),
            doc_from_open.buffer.to_string()
        );
        assert_eq!(doc_from_bytes.encoding, doc_from_open.encoding);
        assert_eq!(doc_from_bytes.line_ending, doc_from_open.line_ending);
        assert_eq!(doc_from_bytes.title, doc_from_open.title);

        std::fs::remove_dir_all(&dir).ok();
    }

    // ── encode_for_save ────────────────────────────────────────────────

    #[test]
    fn test_encode_for_save_utf8() {
        let mut doc = Document::new();
        doc.insert_text("hello");
        doc.encoding = TextEncoding::Utf8;

        let bytes = doc.encode_for_save().unwrap();
        assert_eq!(bytes, b"hello");
    }

    #[test]
    fn test_encode_for_save_applies_crlf() {
        let mut doc = Document::new();
        doc.insert_text("a\nb");
        doc.line_ending = LineEnding::CrLf;

        let bytes = doc.encode_for_save().unwrap();
        assert_eq!(bytes, b"a\r\nb");
    }

    #[test]
    fn test_encode_for_save_utf16le() {
        let mut doc = Document::new();
        doc.insert_text("A");
        doc.encoding = TextEncoding::Utf16Le;

        let bytes = doc.encode_for_save().unwrap();
        // UTF-16 LE BOM + 'A' in UTF-16 LE
        assert_eq!(&bytes[..2], &[0xFF, 0xFE]); // BOM
        assert_eq!(&bytes[2..4], &[0x41, 0x00]); // 'A'
    }

    // ── mark_saved ─────────────────────────────────────────────────────

    #[test]
    fn test_mark_saved_clears_modified_when_version_matches() {
        let mut doc = Document::new();
        doc.insert_text("hello");
        assert!(doc.modified);
        let version = doc.content_version;

        let path = std::path::Path::new("saved.txt");
        doc.mark_saved(path, version);

        assert!(!doc.modified);
        assert_eq!(doc.file_path.as_deref(), Some(path));
        assert_eq!(doc.title, "saved.txt");
        assert!(doc.last_saved_at.is_some());
    }

    #[test]
    fn test_mark_saved_keeps_modified_when_version_differs() {
        let mut doc = Document::new();
        doc.insert_text("hello");
        let old_version = doc.content_version;

        // Simulate further edits
        doc.insert_text(" world");
        assert_ne!(doc.content_version, old_version);

        let path = std::path::Path::new("saved.txt");
        doc.mark_saved(path, old_version);

        // modified should stay true because version changed
        assert!(doc.modified);
        // But path/title/timestamp should still be updated
        assert_eq!(doc.file_path.as_deref(), Some(path));
        assert_eq!(doc.title, "saved.txt");
        assert!(doc.last_saved_at.is_some());
    }

    #[test]
    fn test_mark_saved_updates_line_changes() {
        let mut doc = Document::new();
        doc.insert_text("line1\nline2");
        let version = doc.content_version;

        // Verify some lines are marked as Modified
        let has_modified = doc
            .line_changes
            .iter()
            .any(|c| *c == LineChangeState::Modified);
        assert!(has_modified);

        doc.mark_saved(std::path::Path::new("f.txt"), version);

        // After save, Modified -> Saved
        let has_modified = doc
            .line_changes
            .iter()
            .any(|c| *c == LineChangeState::Modified);
        assert!(!has_modified);
        let has_saved = doc
            .line_changes
            .iter()
            .any(|c| *c == LineChangeState::Saved);
        assert!(has_saved);
    }

    // ── save round-trip ────────────────────────────────────────────────

    #[test]
    fn test_encode_then_from_bytes_roundtrip() {
        let mut doc = Document::new();
        doc.insert_text("hello\nworld");
        doc.encoding = TextEncoding::Utf8;
        doc.line_ending = LineEnding::Lf;

        let bytes = doc.encode_for_save().unwrap();
        let path = std::path::Path::new("round.txt");
        let restored = Document::from_bytes(&bytes, path, None).unwrap();

        assert_eq!(restored.buffer.to_string(), doc.buffer.to_string());
    }

    #[test]
    fn test_encode_then_from_bytes_crlf_roundtrip() {
        let mut doc = Document::new();
        doc.insert_text("hello\nworld");
        doc.encoding = TextEncoding::Utf8;
        doc.line_ending = LineEnding::CrLf;

        let bytes = doc.encode_for_save().unwrap();
        let path = std::path::Path::new("round.txt");
        let restored = Document::from_bytes(&bytes, path, None).unwrap();

        assert_eq!(restored.buffer.to_string(), "hello\nworld");
        assert_eq!(restored.line_ending, LineEnding::CrLf);
    }

    // ── validate_file_size ────────────────────────────────────────────

    #[test]
    fn test_validate_file_size_within_limit() {
        let dir = std::env::temp_dir().join("rust_pad_test_validate_size");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("small.txt");
        std::fs::write(&path, "hello").unwrap();

        let size = validate_file_size(&path, 1024).unwrap();
        assert_eq!(size, 5);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_validate_file_size_exceeds_limit() {
        let dir = std::env::temp_dir().join("rust_pad_test_validate_size_big");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("big.txt");
        std::fs::write(&path, "x".repeat(1024)).unwrap();

        let result = validate_file_size(&path, 100);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("too large"), "Got: {msg}");
        assert!(msg.contains("max_file_size_mb"), "Got: {msg}");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_validate_file_size_exact_limit() {
        let dir = std::env::temp_dir().join("rust_pad_test_validate_exact");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("exact.txt");
        let content = "x".repeat(100);
        std::fs::write(&path, &content).unwrap();

        // Exactly at limit should be OK
        let size = validate_file_size(&path, 100).unwrap();
        assert_eq!(size, 100);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_validate_file_size_nonexistent() {
        let result = validate_file_size(std::path::Path::new("/nonexistent/file"), 1024);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("metadata"));
    }

    #[test]
    fn test_reload_from_disk_respects_size_limit() {
        let dir = std::env::temp_dir().join("rust_pad_test_reload_size");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("size_test.txt");
        std::fs::write(&path, "small").unwrap();

        let mut doc = Document::open(&path).unwrap();

        // Rewrite with larger content
        std::fs::write(&path, "x".repeat(1000)).unwrap();

        // Reload with a limit smaller than the new content
        let result = doc.reload_from_disk(Some(100));
        assert!(result.is_err());
        // Original content should be preserved
        assert_eq!(doc.buffer.to_string(), "small");

        std::fs::remove_dir_all(&dir).ok();
    }
}
