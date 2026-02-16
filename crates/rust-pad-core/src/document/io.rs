//! File I/O operations for documents.
//!
//! Handles opening files from disk (with encoding/line-ending detection),
//! saving documents back to disk, and managing persistent undo history.

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

    /// Internal open shared by `open()` and `open_with_persistence()`.
    fn open_internal(
        path: &std::path::Path,
        persistence: Option<(Arc<PersistenceLayer>, &HistoryConfig)>,
    ) -> Result<Self> {
        let bytes = std::fs::read(path)
            .with_context(|| format!("failed to read file: {}", path.display()))?;

        let encoding = detect_encoding(&bytes);
        let raw_text = decode_bytes(&bytes, encoding)
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

    /// Reloads the document from disk, preserving metadata like live_monitoring.
    ///
    /// Used by live file monitoring to pick up external changes.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be read or decoded.
    pub fn reload_from_disk(&mut self) -> Result<()> {
        let path = self
            .file_path
            .as_ref()
            .context("no file path set for reload")?
            .clone();

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
