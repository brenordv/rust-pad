//! Document model combining text buffer, cursor, and metadata.
//!
//! A `Document` ties together a `TextBuffer`, `Cursor`, undo/redo history,
//! encoding metadata, and UI scroll state. Single-cursor editing operations
//! live here; multi-cursor operations are in the `multi_cursor` submodule,
//! and file I/O is in the `io` submodule.

mod io;
mod multi_cursor;

use std::path::PathBuf;
use std::sync::Arc;

use crate::buffer::TextBuffer;
use crate::cursor::{char_to_pos, pos_to_char, Cursor, Position};
use crate::encoding::{LineEnding, TextEncoding};
use crate::history::{
    generate_unsaved_id, CursorSnapshot, EditOperation, HistoryConfig, PersistenceLayer,
    UndoManager,
};
use crate::indent::IndentStyle;

/// Converts a `Position` to a `CursorSnapshot` for history recording.
fn snap(pos: Position) -> CursorSnapshot {
    CursorSnapshot {
        line: pos.line,
        col: pos.col,
    }
}

/// Snapshot of buffer and cursor state captured before a bulk edit.
///
/// Used with [`Document::record_undo_from_snapshot`] to make bulk operations
/// (sort lines, remove duplicates, etc.) undoable.
pub struct UndoSnapshot {
    content: String,
    cursor: Position,
}

/// Change tracking state for a line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineChangeState {
    /// Line has not been modified since last save.
    Unchanged,
    /// Line was modified but not yet saved.
    Modified,
    /// Line was modified and has been saved.
    Saved,
}

/// Tracks which scrollbar is being actively dragged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ScrollbarDrag {
    /// No scrollbar is being dragged.
    #[default]
    None,
    /// The vertical scrollbar is being dragged.
    Vertical,
    /// The horizontal scrollbar is being dragged.
    Horizontal,
}

/// A single document with its buffer, cursor, history, and metadata.
pub struct Document {
    /// The text buffer.
    pub buffer: TextBuffer,
    /// The cursor state.
    pub cursor: Cursor,
    /// Additional cursors for multi-cursor editing.
    pub secondary_cursors: Vec<Cursor>,
    /// Undo/redo history manager.
    pub history: UndoManager,
    /// File path on disk, if any.
    pub file_path: Option<PathBuf>,
    /// The encoding used for this document.
    pub encoding: TextEncoding,
    /// The line ending style.
    pub line_ending: LineEnding,
    /// The indentation style.
    pub indent_style: IndentStyle,
    /// Whether the document has been modified since last save.
    pub modified: bool,
    /// Display name for the tab.
    pub title: String,
    /// Per-line change tracking state.
    pub line_changes: Vec<LineChangeState>,
    /// Scroll offset (line index of the top visible line).
    pub scroll_y: f32,
    /// Horizontal scroll offset in pixels.
    pub scroll_x: f32,
    /// Timestamp of last cursor activity (for blink reset), in egui time seconds.
    /// After activity the cursor stays solid for a short period before blinking.
    pub cursor_activity_time: f64,
    /// Which scrollbar is currently being dragged, if any.
    pub scrollbar_drag: ScrollbarDrag,
    /// Links unsaved tabs to session store content for restore-on-startup.
    pub session_id: Option<String>,
    /// Flag requesting the UI to scroll the viewport so the cursor is visible.
    ///
    /// Set by operations that move the cursor outside the editor widget's
    /// input loop (e.g. paste, undo/redo from global shortcuts). The widget
    /// clears this after honoring it.
    pub scroll_to_cursor: bool,
    /// Timestamp of the last successful save to disk.
    pub last_saved_at: Option<chrono::DateTime<chrono::Local>>,
    /// Whether live file monitoring is active (auto-reload on external changes).
    pub live_monitoring: bool,
    /// Last known file modification time, for change detection.
    pub last_known_mtime: Option<std::time::SystemTime>,
    /// Monotonically increasing version counter, bumped on every buffer mutation.
    /// Used by UI caches (wrap map, occurrence highlights, etc.) to detect changes
    /// without comparing content.
    pub content_version: u64,
    /// Cached max line length in chars: `(content_version, value)`.
    pub cached_max_line_chars: Option<(u64, usize)>,
    /// Cached occurrence highlight ranges: `(content_version, needle, ranges)`.
    #[allow(clippy::type_complexity)]
    pub cached_occurrences: Option<(u64, String, Vec<(usize, usize)>)>,
    /// Opaque storage for UI-layer render caches (galley cache, etc.).
    ///
    /// Typed as `Box<dyn Any + Send>` so that the core crate doesn't depend on egui types.
    /// The UI layer downcasts this to its concrete `RenderCache` struct.
    pub render_cache: Option<Box<dyn std::any::Any + Send>>,
}

impl std::fmt::Debug for Document {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Document")
            .field("buffer", &self.buffer)
            .field("cursor", &self.cursor)
            .field("file_path", &self.file_path)
            .field("encoding", &self.encoding)
            .field("line_ending", &self.line_ending)
            .field("modified", &self.modified)
            .field("title", &self.title)
            .field("content_version", &self.content_version)
            .finish_non_exhaustive()
    }
}

impl Default for Document {
    fn default() -> Self {
        Self::new()
    }
}

impl Document {
    /// Creates a new empty document with in-memory-only history.
    pub fn new() -> Self {
        Self::new_internal(None)
    }

    /// Creates a new empty document with persistent history.
    pub fn with_persistence(persistence: Arc<PersistenceLayer>, config: &HistoryConfig) -> Self {
        Self::new_internal(Some((persistence, config)))
    }

    /// Internal constructor shared by `new()` and `with_persistence()`.
    fn new_internal(persistence: Option<(Arc<PersistenceLayer>, &HistoryConfig)>) -> Self {
        let history = match persistence {
            Some((pl, config)) => {
                let doc_id = generate_unsaved_id();
                UndoManager::new(doc_id, config.clone(), Some(pl))
            }
            None => UndoManager::in_memory(),
        };
        Self {
            buffer: TextBuffer::new(),
            cursor: Cursor::new(),
            secondary_cursors: Vec::new(),
            history,
            file_path: None,
            encoding: TextEncoding::default(),
            line_ending: LineEnding::default(),
            indent_style: IndentStyle::default(),
            modified: false,
            title: "Untitled".to_string(),
            line_changes: vec![LineChangeState::Unchanged],
            scroll_y: 0.0,
            scroll_x: 0.0,
            cursor_activity_time: 0.0,
            scrollbar_drag: ScrollbarDrag::None,
            session_id: None,
            scroll_to_cursor: false,
            last_saved_at: None,
            live_monitoring: false,
            last_known_mtime: None,
            content_version: 0,
            cached_max_line_chars: None,
            cached_occurrences: None,
            render_cache: None,
        }
    }

    /// Bumps the content version counter.
    ///
    /// Called by every method that mutates the buffer so that UI caches
    /// can detect changes without comparing content.
    #[inline]
    fn bump_version(&mut self) {
        self.content_version = self.content_version.wrapping_add(1);
    }

    /// Inserts text at the current cursor position.
    pub fn insert_text(&mut self, text: &str) {
        // Delete selection first if any
        if self.cursor.selection_anchor.is_some() {
            self.delete_selection();
        }

        let char_idx = self.cursor.to_char_index(&self.buffer).unwrap_or(0);
        let cursor_before = self.cursor.position;

        if self.buffer.insert(char_idx, text).is_ok() {
            // Advance cursor past inserted text
            let new_pos = char_to_pos(&self.buffer, char_idx + text.chars().count());
            self.cursor.position = new_pos;
            self.cursor.desired_col = None;

            self.history.record(EditOperation {
                position: char_idx,
                inserted: text.to_string(),
                deleted: String::new(),
                cursor_before: snap(cursor_before),
                cursor_after: snap(self.cursor.position),
            });

            self.mark_lines_modified(cursor_before.line, self.cursor.position.line);
            self.modified = true;
            self.scroll_to_cursor = true;
            self.bump_version();
        }
    }

    /// Inserts a newline at the current cursor position.
    pub fn insert_newline(&mut self) {
        self.insert_text("\n");
    }

    /// Deletes the selected text.
    pub fn delete_selection(&mut self) {
        if let Ok(Some((start, end))) = self.cursor.selection_char_range(&self.buffer) {
            if start == end {
                self.cursor.clear_selection();
                return;
            }

            let cursor_before = self.cursor.position;
            let deleted_text = self
                .buffer
                .slice(start, end)
                .map(|s| s.to_string())
                .unwrap_or_default();

            let start_pos = char_to_pos(&self.buffer, start);
            let end_line = char_to_pos(&self.buffer, end).line;

            if self.buffer.remove(start, end).is_ok() {
                self.cursor.position = start_pos;
                self.cursor.clear_selection();
                self.cursor.desired_col = None;

                self.history.record(EditOperation {
                    position: start,
                    inserted: String::new(),
                    deleted: deleted_text,
                    cursor_before: snap(cursor_before),
                    cursor_after: snap(self.cursor.position),
                });

                self.mark_lines_modified(start_pos.line, end_line);
                self.sync_line_changes();
                self.modified = true;
                self.scroll_to_cursor = true;
                self.bump_version();
            }
        }
    }

    /// Deletes the character before the cursor (backspace).
    pub fn backspace(&mut self) {
        if self.cursor.selection_anchor.is_some() {
            self.delete_selection();
            return;
        }

        let char_idx = self.cursor.to_char_index(&self.buffer).unwrap_or(0);
        if char_idx == 0 {
            return;
        }

        let cursor_before = self.cursor.position;
        let deleted_char = self.buffer.char_at(char_idx - 1).unwrap_or(' ');
        let deleted = deleted_char.to_string();

        if self.buffer.remove(char_idx - 1, char_idx).is_ok() {
            self.cursor.position = char_to_pos(&self.buffer, char_idx - 1);
            self.cursor.desired_col = None;

            self.history.record(EditOperation {
                position: char_idx - 1,
                inserted: String::new(),
                deleted,
                cursor_before: snap(cursor_before),
                cursor_after: snap(self.cursor.position),
            });

            self.mark_lines_modified(self.cursor.position.line, cursor_before.line);
            self.sync_line_changes();
            self.modified = true;
            self.bump_version();
        }
    }

    /// Deletes the character after the cursor (delete key).
    pub fn delete_forward(&mut self) {
        if self.cursor.selection_anchor.is_some() {
            self.delete_selection();
            return;
        }

        let char_idx = self.cursor.to_char_index(&self.buffer).unwrap_or(0);
        if char_idx >= self.buffer.len_chars() {
            return;
        }

        let cursor_before = self.cursor.position;
        let deleted_char = self.buffer.char_at(char_idx).unwrap_or(' ');
        let deleted = deleted_char.to_string();

        if self.buffer.remove(char_idx, char_idx + 1).is_ok() {
            self.history.record(EditOperation {
                position: char_idx,
                inserted: String::new(),
                deleted,
                cursor_before: snap(cursor_before),
                cursor_after: snap(self.cursor.position),
            });

            self.mark_lines_modified(self.cursor.position.line, self.cursor.position.line);
            self.sync_line_changes();
            self.modified = true;
            self.bump_version();
        }
    }

    /// Performs undo.
    pub fn undo(&mut self) {
        if let Some(ops) = self.history.undo() {
            self.history.pause_recording();
            // Apply operations in reverse
            for op in ops.iter().rev() {
                // Reverse: remove what was inserted, insert what was deleted
                if !op.inserted.is_empty() {
                    let _ = self
                        .buffer
                        .remove(op.position, op.position + op.inserted.chars().count());
                }
                if !op.deleted.is_empty() {
                    let _ = self.buffer.insert(op.position, &op.deleted);
                }
            }
            // Restore cursor to before position
            if let Some(first_op) = ops.first() {
                self.cursor.position = first_op.cursor_before.into();
                self.cursor.clear_selection();
                self.cursor.desired_col = None;
            }
            self.history.resume_recording();
            self.sync_line_changes();
            self.modified = true;
            self.scroll_to_cursor = true;
            self.bump_version();
        }
    }

    /// Performs redo.
    pub fn redo(&mut self) {
        if let Some(ops) = self.history.redo() {
            self.history.pause_recording();
            // Apply operations in forward order
            for op in &ops {
                if !op.deleted.is_empty() {
                    let _ = self
                        .buffer
                        .remove(op.position, op.position + op.deleted.chars().count());
                }
                if !op.inserted.is_empty() {
                    let _ = self.buffer.insert(op.position, &op.inserted);
                }
            }
            // Restore cursor to after position
            if let Some(last_op) = ops.last() {
                self.cursor.position = last_op.cursor_after.into();
                self.cursor.clear_selection();
                self.cursor.desired_col = None;
            }
            self.history.resume_recording();
            self.sync_line_changes();
            self.modified = true;
            self.scroll_to_cursor = true;
            self.bump_version();
        }
    }

    /// Returns the selected text, or None if no selection.
    pub fn selected_text(&self) -> Option<String> {
        if let Ok(Some((start, end))) = self.cursor.selection_char_range(&self.buffer) {
            if start == end {
                return None;
            }
            self.buffer.slice(start, end).ok().map(|s| s.to_string())
        } else {
            None
        }
    }

    /// Returns true if multi-cursor mode is active.
    pub fn is_multi_cursor(&self) -> bool {
        !self.secondary_cursors.is_empty()
    }

    /// Clears all secondary cursors, returning to single-cursor mode.
    pub fn clear_secondary_cursors(&mut self) {
        self.secondary_cursors.clear();
    }

    /// Adds a secondary cursor, merging overlapping ones.
    pub fn add_secondary_cursor(&mut self, cursor: Cursor) {
        self.secondary_cursors.push(cursor);
        self.merge_overlapping_cursors();
    }

    /// Merges cursors that overlap or are at the same position.
    pub(crate) fn merge_overlapping_cursors(&mut self) {
        // Collect all cursor positions (as char indices) to detect duplicates
        let primary_idx = pos_to_char(&self.buffer, self.cursor.position).unwrap_or(0);
        self.secondary_cursors.retain(|sc| {
            let sc_idx = pos_to_char(&self.buffer, sc.position).unwrap_or(0);
            sc_idx != primary_idx
        });

        // Deduplicate secondary cursors by position
        let mut seen = vec![primary_idx];
        self.secondary_cursors.retain(|sc| {
            let idx = pos_to_char(&self.buffer, sc.position).unwrap_or(0);
            if seen.contains(&idx) {
                false
            } else {
                seen.push(idx);
                true
            }
        });
    }

    /// Deletes the current line (Ctrl+D).
    pub fn delete_line(&mut self) {
        let line = self.cursor.position.line;
        let total_lines = self.buffer.len_lines();
        let line_start = self.buffer.line_to_char(line).unwrap_or(0);

        let end = if total_lines <= 1 {
            // Only line: delete everything
            self.buffer.len_chars()
        } else if line + 1 < total_lines {
            // Not last line: delete line including its trailing newline
            self.buffer.line_to_char(line + 1).unwrap_or(line_start)
        } else {
            // Last line: also remove the preceding newline
            self.buffer.len_chars()
        };

        // For last line, start from end of previous line's content (eat preceding newline)
        let start = if line + 1 >= total_lines && line > 0 {
            let prev_line_end = self.buffer.line_to_char(line).unwrap_or(0);
            prev_line_end.saturating_sub(1) // eat the \n before this line
        } else {
            line_start
        };

        if start == end {
            return;
        }

        let deleted_text = self
            .buffer
            .slice(start, end)
            .map(|s| s.to_string())
            .unwrap_or_default();
        let cursor_before = self.cursor.position;

        if self.buffer.remove(start, end).is_ok() {
            let new_line = line.min(self.buffer.len_lines().saturating_sub(1));
            let line_len = self.buffer.line_len_chars(new_line).unwrap_or(0);
            self.cursor.position = Position::new(new_line, self.cursor.position.col.min(line_len));
            self.cursor.clear_selection();

            self.history.record(EditOperation {
                position: start,
                inserted: String::new(),
                deleted: deleted_text,
                cursor_before: snap(cursor_before),
                cursor_after: snap(self.cursor.position),
            });

            self.sync_line_changes();
            self.modified = true;
            self.scroll_to_cursor = true;
            self.bump_version();
        }
    }

    /// Marks lines as modified for change tracking.
    fn mark_lines_modified(&mut self, from_line: usize, to_line: usize) {
        let start = from_line.min(to_line);
        let end = from_line.max(to_line);
        for line_idx in start..=end {
            if line_idx < self.line_changes.len() {
                self.line_changes[line_idx] = LineChangeState::Modified;
            }
        }
    }

    /// Captures a snapshot of the current buffer and cursor for undo recording.
    ///
    /// Call this before a bulk operation, then call
    /// [`record_undo_from_snapshot`](Self::record_undo_from_snapshot) after
    /// to make the operation undoable in a single step.
    pub fn snapshot_for_undo(&self) -> UndoSnapshot {
        UndoSnapshot {
            content: self.buffer.to_string(),
            cursor: self.cursor.position,
        }
    }

    /// Records a single undo entry from a pre-operation snapshot.
    ///
    /// Compares the current buffer to the snapshot and records the difference
    /// as one operation that replaces the entire content. Does nothing if the
    /// buffer is unchanged.
    pub fn record_undo_from_snapshot(&mut self, snapshot: UndoSnapshot) {
        let new_content = self.buffer.to_string();
        if new_content == snapshot.content {
            return;
        }
        self.history.force_group_break();
        self.history.record(EditOperation {
            position: 0,
            deleted: snapshot.content,
            inserted: new_content,
            cursor_before: snap(snapshot.cursor),
            cursor_after: snap(self.cursor.position),
        });
        self.history.force_group_break();
        self.sync_line_changes();
        self.modified = true;
        self.scroll_to_cursor = true;
        self.bump_version();
    }

    /// Syncs the line_changes vector length with the buffer's line count.
    pub(crate) fn sync_line_changes(&mut self) {
        let line_count = self.buffer.len_lines().max(1);
        self.line_changes
            .resize(line_count, LineChangeState::Modified);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cursor::Position;

    // ── Content version counter tests ─────────────────────────────

    #[test]
    fn test_content_version_starts_at_zero() {
        let doc = Document::new();
        assert_eq!(doc.content_version, 0);
    }

    #[test]
    fn test_content_version_increments_on_insert() {
        let mut doc = Document::new();
        assert_eq!(doc.content_version, 0);
        doc.insert_text("hello");
        assert_eq!(doc.content_version, 1);
        doc.insert_text(" world");
        assert_eq!(doc.content_version, 2);
    }

    #[test]
    fn test_content_version_increments_on_backspace() {
        let mut doc = Document::new();
        doc.insert_text("hi");
        let v = doc.content_version;
        doc.backspace();
        assert_eq!(doc.content_version, v + 1);
    }

    #[test]
    fn test_content_version_increments_on_delete_forward() {
        let mut doc = Document::new();
        doc.insert_text("hi");
        doc.cursor.position = Position::new(0, 0);
        let v = doc.content_version;
        doc.delete_forward();
        assert_eq!(doc.content_version, v + 1);
    }

    #[test]
    fn test_content_version_increments_on_undo_redo() {
        let mut doc = Document::new();
        doc.insert_text("hello");
        doc.history.force_group_break();
        let v = doc.content_version;
        doc.undo();
        assert_eq!(doc.content_version, v + 1);
        let v2 = doc.content_version;
        doc.redo();
        assert_eq!(doc.content_version, v2 + 1);
    }

    #[test]
    fn test_content_version_increments_on_delete_selection() {
        let mut doc = Document::new();
        doc.insert_text("hello");
        doc.cursor.position = Position::new(0, 0);
        doc.cursor.start_selection();
        doc.cursor.position = Position::new(0, 3);
        let v = doc.content_version;
        doc.delete_selection();
        assert_eq!(doc.content_version, v + 1);
    }

    #[test]
    fn test_content_version_increments_on_delete_line() {
        let mut doc = Document::new();
        doc.insert_text("a\nb\nc");
        let v = doc.content_version;
        doc.cursor.position = Position::new(1, 0);
        doc.delete_line();
        assert_eq!(doc.content_version, v + 1);
    }

    #[test]
    fn test_new_document() {
        let doc = Document::new();
        assert!(doc.buffer.is_empty());
        assert_eq!(doc.title, "Untitled");
        assert!(!doc.modified);
        assert_eq!(doc.indent_style, IndentStyle::default());
        assert_eq!(doc.indent_style, IndentStyle::Spaces(4));
    }

    #[test]
    fn test_insert_text() {
        let mut doc = Document::new();
        doc.insert_text("hello");
        assert_eq!(doc.buffer.to_string(), "hello");
        assert_eq!(doc.cursor.position, Position::new(0, 5));
        assert!(doc.modified);
    }

    #[test]
    fn test_backspace() {
        let mut doc = Document::new();
        doc.insert_text("hello");
        doc.backspace();
        assert_eq!(doc.buffer.to_string(), "hell");
        assert_eq!(doc.cursor.position, Position::new(0, 4));
    }

    #[test]
    fn test_delete_forward() {
        let mut doc = Document::new();
        doc.insert_text("hello");
        doc.cursor.position = Position::new(0, 0);
        doc.delete_forward();
        assert_eq!(doc.buffer.to_string(), "ello");
    }

    #[test]
    fn test_undo_redo() {
        let mut doc = Document::new();
        doc.insert_text("hello");
        doc.history.force_group_break();
        doc.insert_text(" world");
        assert_eq!(doc.buffer.to_string(), "hello world");

        doc.undo();
        assert_eq!(doc.buffer.to_string(), "hello");

        doc.redo();
        assert_eq!(doc.buffer.to_string(), "hello world");
    }

    #[test]
    fn test_delete_selection() {
        let mut doc = Document::new();
        doc.insert_text("hello world");
        doc.cursor.position = Position::new(0, 0);
        doc.cursor.start_selection();
        doc.cursor.position = Position::new(0, 5);
        doc.delete_selection();
        assert_eq!(doc.buffer.to_string(), " world");
    }

    #[test]
    fn test_selected_text() {
        let mut doc = Document::new();
        doc.insert_text("hello world");
        doc.cursor.position = Position::new(0, 0);
        doc.cursor.start_selection();
        doc.cursor.position = Position::new(0, 5);
        assert_eq!(doc.selected_text(), Some("hello".to_string()));
    }

    #[test]
    fn test_insert_newline_no_auto_indent() {
        let mut doc = Document::new();
        doc.insert_text("    hello");
        doc.insert_newline();
        assert_eq!(doc.buffer.to_string(), "    hello\n");
        assert_eq!(doc.cursor.position, Position::new(1, 0));
    }

    #[test]
    fn test_save_roundtrip() {
        let dir = std::env::temp_dir().join("rust_pad_test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("test_save.txt");

        let mut doc = Document::new();
        doc.insert_text("hello world");
        doc.file_path = Some(path.clone());
        doc.save().unwrap();

        let loaded = Document::open(&path).unwrap();
        assert_eq!(loaded.buffer.to_string(), "hello world");
        assert!(!loaded.modified);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_change_tracking() {
        let mut doc = Document::new();
        doc.insert_text("hello");
        assert_eq!(doc.line_changes[0], LineChangeState::Modified);
    }

    // ── Indent style tests ─────────────────────────────────────────────

    #[test]
    fn test_indent_style_default_is_spaces_4() {
        let doc = Document::new();
        assert_eq!(doc.indent_style, IndentStyle::Spaces(4));
        assert_eq!(doc.indent_style.indent_text(), "    ");
    }

    #[test]
    fn test_indent_style_can_be_changed() {
        let mut doc = Document::new();
        doc.indent_style = IndentStyle::Tabs;
        assert_eq!(doc.indent_style.indent_text(), "\t");
        doc.indent_style = IndentStyle::Spaces(2);
        assert_eq!(doc.indent_style.indent_text(), "  ");
    }

    #[test]
    fn test_open_detects_tab_indented_file() {
        let dir = std::env::temp_dir().join("rust_pad_indent_test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("tabs.rs");
        std::fs::write(&path, "fn main() {\n\tprintln!(\"hello\");\n}\n").unwrap();

        let doc = Document::open(&path).unwrap();
        assert_eq!(doc.indent_style, IndentStyle::Tabs);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_open_detects_2_space_indented_file() {
        let dir = std::env::temp_dir().join("rust_pad_indent_test_2sp");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("two_space.js");
        std::fs::write(
            &path,
            "function main() {\n  const x = 1;\n  if (x) {\n    inner();\n  }\n}\n",
        )
        .unwrap();

        let doc = Document::open(&path).unwrap();
        assert_eq!(doc.indent_style, IndentStyle::Spaces(2));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_open_detects_4_space_indented_file() {
        let dir = std::env::temp_dir().join("rust_pad_indent_test_4sp");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("four_space.rs");
        std::fs::write(
            &path,
            "fn main() {\n    let x = 1;\n    if x > 0 {\n        inner();\n    }\n}\n",
        )
        .unwrap();

        let doc = Document::open(&path).unwrap();
        assert_eq!(doc.indent_style, IndentStyle::Spaces(4));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_open_no_indentation_defaults_to_spaces_4() {
        let dir = std::env::temp_dir().join("rust_pad_indent_test_none");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("flat.txt");
        std::fs::write(&path, "line1\nline2\nline3\n").unwrap();

        let doc = Document::open(&path).unwrap();
        assert_eq!(doc.indent_style, IndentStyle::Spaces(4));

        std::fs::remove_dir_all(&dir).ok();
    }

    // ── Delete line tests ────────────────────────────────────────────

    #[test]
    fn test_delete_line_single() {
        let mut doc = Document::new();
        doc.insert_text("line1\nline2\nline3");
        doc.cursor.position = Position::new(1, 0);
        doc.delete_line();
        assert_eq!(doc.buffer.to_string(), "line1\nline3");
        assert_eq!(doc.cursor.position.line, 1);
        assert!(doc.modified);
    }

    #[test]
    fn test_delete_line_last() {
        let mut doc = Document::new();
        doc.insert_text("line1\nline2");
        doc.cursor.position = Position::new(1, 3);
        doc.delete_line();
        assert_eq!(doc.buffer.to_string(), "line1");
        assert_eq!(doc.cursor.position.line, 0);
    }

    #[test]
    fn test_delete_line_only() {
        let mut doc = Document::new();
        doc.insert_text("only line");
        doc.cursor.position = Position::new(0, 3);
        doc.delete_line();
        assert_eq!(doc.buffer.to_string(), "");
    }

    #[test]
    fn test_delete_line_first() {
        let mut doc = Document::new();
        doc.insert_text("first\nsecond\nthird");
        doc.cursor.position = Position::new(0, 0);
        doc.delete_line();
        assert_eq!(doc.buffer.to_string(), "second\nthird");
        assert_eq!(doc.cursor.position.line, 0);
    }

    // ── Multi-cursor helper tests ────────────────────────────────────

    #[test]
    fn test_is_multi_cursor() {
        let mut doc = Document::new();
        assert!(!doc.is_multi_cursor());
        let mut sc = Cursor::new();
        sc.position = Position::new(0, 5);
        doc.secondary_cursors.push(sc);
        assert!(doc.is_multi_cursor());
    }

    #[test]
    fn test_clear_secondary_cursors() {
        let mut doc = Document::new();
        doc.insert_text("hello world");
        let mut sc = Cursor::new();
        sc.position = Position::new(0, 5);
        doc.secondary_cursors.push(sc);
        assert!(doc.is_multi_cursor());
        doc.clear_secondary_cursors();
        assert!(!doc.is_multi_cursor());
    }

    #[test]
    fn test_add_secondary_cursor() {
        let mut doc = Document::new();
        doc.insert_text("hello world");
        doc.cursor.position = Position::new(0, 0);
        let mut sc = Cursor::new();
        sc.position = Position::new(0, 6);
        doc.add_secondary_cursor(sc);
        assert_eq!(doc.secondary_cursors.len(), 1);
        assert_eq!(doc.secondary_cursors[0].position, Position::new(0, 6));
    }

    #[test]
    fn test_multi_cursor_merge_overlap() {
        let mut doc = Document::new();
        doc.insert_text("hello world");
        doc.cursor.position = Position::new(0, 0);
        // Add cursor at same position as primary — should be merged
        let sc = Cursor::new(); // position (0, 0) same as primary
        doc.add_secondary_cursor(sc);
        assert_eq!(doc.secondary_cursors.len(), 0);
    }

    // ── Multi-cursor insert tests ────────────────────────────────────

    #[test]
    fn test_multi_cursor_insert() {
        let mut doc = Document::new();
        doc.insert_text("hello\nworld\nfoo");
        // Primary cursor at start of line 0
        doc.cursor.position = Position::new(0, 0);
        // Add cursors at start of lines 1 and 2
        let mut sc1 = Cursor::new();
        sc1.position = Position::new(1, 0);
        doc.secondary_cursors.push(sc1);
        let mut sc2 = Cursor::new();
        sc2.position = Position::new(2, 0);
        doc.secondary_cursors.push(sc2);

        doc.insert_text_multi("X");
        assert_eq!(doc.buffer.to_string(), "Xhello\nXworld\nXfoo");
    }

    #[test]
    fn test_multi_cursor_backspace() {
        let mut doc = Document::new();
        doc.insert_text("Xhello\nXworld\nXfoo");
        // Cursors after the X on each line
        doc.cursor.position = Position::new(0, 1);
        let mut sc1 = Cursor::new();
        sc1.position = Position::new(1, 1);
        doc.secondary_cursors.push(sc1);
        let mut sc2 = Cursor::new();
        sc2.position = Position::new(2, 1);
        doc.secondary_cursors.push(sc2);

        doc.backspace_multi();
        assert_eq!(doc.buffer.to_string(), "hello\nworld\nfoo");
    }

    #[test]
    fn test_selected_text_multi() {
        let mut doc = Document::new();
        doc.insert_text("hello world foo");
        // Select "hello" with primary cursor
        doc.cursor.position = Position::new(0, 5);
        doc.cursor.selection_anchor = Some(Position::new(0, 0));
        // Select "world" with secondary cursor
        let mut sc = Cursor::new();
        sc.position = Position::new(0, 11);
        sc.selection_anchor = Some(Position::new(0, 6));
        doc.secondary_cursors.push(sc);

        let text = doc.selected_text_multi().unwrap();
        assert_eq!(text, "hello\nworld");
    }

    // ── Per-cursor insert tests (paste distribution) ────────────────

    #[test]
    fn test_insert_text_per_cursor_basic() {
        let mut doc = Document::new();
        doc.insert_text("AAA\nBBB\nCCC");
        // Primary cursor at end of line 0
        doc.cursor.position = Position::new(0, 3);
        // Secondary cursors at end of lines 1 and 2
        let mut sc1 = Cursor::new();
        sc1.position = Position::new(1, 3);
        doc.secondary_cursors.push(sc1);
        let mut sc2 = Cursor::new();
        sc2.position = Position::new(2, 3);
        doc.secondary_cursors.push(sc2);

        doc.insert_text_per_cursor(&["X", "Y", "Z"]);
        assert_eq!(doc.buffer.to_string(), "AAAX\nBBBY\nCCCZ");
    }

    #[test]
    fn test_insert_text_per_cursor_preserves_cursor_positions() {
        let mut doc = Document::new();
        doc.insert_text("aa\nbb\ncc");
        doc.cursor.position = Position::new(0, 2);
        let mut sc = Cursor::new();
        sc.position = Position::new(1, 2);
        doc.secondary_cursors.push(sc);

        doc.insert_text_per_cursor(&["11", "22"]);
        assert_eq!(doc.buffer.to_string(), "aa11\nbb22\ncc");
        // Primary cursor should be after "11"
        assert_eq!(doc.cursor.position, Position::new(0, 4));
        // Secondary cursor should be after "22"
        assert_eq!(doc.secondary_cursors[0].position, Position::new(1, 4));
    }

    #[test]
    fn test_insert_text_per_cursor_mismatched_count_falls_back() {
        let mut doc = Document::new();
        doc.insert_text("aa\nbb");
        doc.cursor.position = Position::new(0, 2);
        let mut sc = Cursor::new();
        sc.position = Position::new(1, 2);
        doc.secondary_cursors.push(sc);

        // 3 texts but only 2 cursors: falls back to insert_text_multi with first text
        doc.insert_text_per_cursor(&["X", "Y", "Z"]);
        // Fallback inserts "X" at both cursors
        assert_eq!(doc.buffer.to_string(), "aaX\nbbX");
    }

    #[test]
    fn test_insert_text_per_cursor_roundtrip_copy_paste() {
        // Simulate: select words on different lines, copy, paste back
        let mut doc = Document::new();
        doc.insert_text("foo bar\nbaz qux");
        // Select "foo" with primary cursor
        doc.cursor.selection_anchor = Some(Position::new(0, 0));
        doc.cursor.position = Position::new(0, 3);
        // Select "baz" with secondary cursor
        let mut sc = Cursor::new();
        sc.selection_anchor = Some(Position::new(1, 0));
        sc.position = Position::new(1, 3);
        doc.secondary_cursors.push(sc);

        // Copy (simulated)
        let copied = doc.selected_text_multi().unwrap();
        assert_eq!(copied, "foo\nbaz");

        // Delete selections (simulates cut)
        doc.delete_selection_multi_public();

        // Buffer should now be " bar\n qux"
        assert_eq!(doc.buffer.to_string(), " bar\n qux");

        // Paste back — split by \n, one per cursor
        let lines: Vec<&str> = copied.split('\n').collect();
        doc.insert_text_per_cursor(&lines);
        assert_eq!(doc.buffer.to_string(), "foo bar\nbaz qux");
    }

    // ── insert_newline cursor position tests ────────────────────────

    #[test]
    fn test_newline_at_end_of_indented_line() {
        let mut doc = Document::new();
        doc.insert_text("    hello");
        assert_eq!(doc.cursor.position, Position::new(0, 9));
        doc.insert_newline();
        assert_eq!(doc.buffer.to_string(), "    hello\n");
        assert_eq!(doc.cursor.position, Position::new(1, 0));
    }

    #[test]
    fn test_newline_at_end_of_unindented_line() {
        let mut doc = Document::new();
        doc.insert_text("hello");
        assert_eq!(doc.cursor.position, Position::new(0, 5));
        doc.insert_newline();
        assert_eq!(doc.buffer.to_string(), "hello\n");
        assert_eq!(doc.cursor.position, Position::new(1, 0));
    }

    #[test]
    fn test_newline_in_middle_of_indented_line() {
        let mut doc = Document::new();
        doc.insert_text("    hello world");
        doc.cursor.position = Position::new(0, 9);
        doc.insert_newline();
        assert_eq!(doc.buffer.to_string(), "    hello\n world");
        assert_eq!(doc.cursor.position, Position::new(1, 0));
    }

    #[test]
    fn test_newline_at_start_of_line() {
        let mut doc = Document::new();
        doc.insert_text("    hello");
        doc.cursor.position = Position::new(0, 0);
        doc.insert_newline();
        assert_eq!(doc.buffer.to_string(), "\n    hello");
        assert_eq!(doc.cursor.position, Position::new(1, 0));
    }

    #[test]
    fn test_newline_on_empty_document() {
        let mut doc = Document::new();
        assert_eq!(doc.cursor.position, Position::new(0, 0));
        doc.insert_newline();
        assert_eq!(doc.buffer.to_string(), "\n");
        assert_eq!(doc.cursor.position, Position::new(1, 0));
    }

    #[test]
    fn test_newline_no_tab_indent_copy() {
        let mut doc = Document::new();
        doc.insert_text("\thello");
        assert_eq!(doc.cursor.position, Position::new(0, 6));
        doc.insert_newline();
        assert_eq!(doc.buffer.to_string(), "\thello\n");
        assert_eq!(doc.cursor.position, Position::new(1, 0));
    }

    #[test]
    fn test_newline_between_existing_lines() {
        let mut doc = Document::new();
        doc.insert_text("    line1\n    line2");
        doc.cursor.position = Position::new(0, 9);
        doc.insert_newline();
        assert_eq!(doc.buffer.to_string(), "    line1\n\n    line2");
        assert_eq!(doc.cursor.position, Position::new(1, 0));
    }

    #[test]
    fn test_newline_multiple_times() {
        let mut doc = Document::new();
        doc.insert_text("    code");
        doc.insert_newline();
        assert_eq!(doc.cursor.position, Position::new(1, 0));
        doc.insert_newline();
        assert_eq!(doc.cursor.position, Position::new(2, 0));
        assert_eq!(doc.buffer.to_string(), "    code\n\n");
    }

    #[test]
    fn test_newline_with_selection_deletes_first() {
        let mut doc = Document::new();
        doc.insert_text("    hello world");
        // Select "world" (positions 10-15)
        doc.cursor.position = Position::new(0, 15);
        doc.cursor.selection_anchor = Some(Position::new(0, 10));
        doc.insert_newline();
        // "world" deleted, then plain newline inserted (no auto-indent)
        assert_eq!(doc.buffer.to_string(), "    hello \n");
        assert_eq!(doc.cursor.position, Position::new(1, 0));
    }

    // ── session_id ───────────────────────────────────────────────────

    #[test]
    fn test_session_id_default_none() {
        let doc = Document::new();
        assert!(doc.session_id.is_none());
    }

    #[test]
    fn test_session_id_can_be_set() {
        let mut doc = Document::new();
        doc.session_id = Some("sess-42".to_string());
        assert_eq!(doc.session_id.as_deref(), Some("sess-42"));
    }

    // ── save without path ────────────────────────────────────────────

    #[test]
    fn test_save_without_path_errors() {
        let mut doc = Document::new();
        doc.insert_text("hello");
        let result = doc.save();
        assert!(result.is_err());
    }

    // ── open nonexistent file ────────────────────────────────────────

    #[test]
    fn test_open_nonexistent_file() {
        let result = Document::open(std::path::Path::new("/nonexistent/path/file.txt"));
        assert!(result.is_err());
    }

    // ── save_to updates metadata ─────────────────────────────────────

    #[test]
    fn test_save_to_updates_path_and_title() {
        let dir = std::env::temp_dir().join("rust_pad_test_save_to");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("new_file.txt");

        let mut doc = Document::new();
        doc.insert_text("content");
        doc.save_to(&path).unwrap();

        assert_eq!(doc.file_path.as_deref(), Some(path.as_path()));
        assert_eq!(doc.title, "new_file.txt");
        assert!(!doc.modified);

        std::fs::remove_dir_all(&dir).ok();
    }

    // ── change tracking after save ───────────────────────────────────

    #[test]
    fn test_change_tracking_saved_state() {
        let dir = std::env::temp_dir().join("rust_pad_test_change_saved");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("change_test.txt");

        let mut doc = Document::new();
        doc.insert_text("hello");
        assert_eq!(doc.line_changes[0], LineChangeState::Modified);

        doc.file_path = Some(path.clone());
        doc.save().unwrap();

        // After save, modified lines become saved
        assert_eq!(doc.line_changes[0], LineChangeState::Saved);

        std::fs::remove_dir_all(&dir).ok();
    }

    // ── backspace at start of buffer ─────────────────────────────────

    #[test]
    fn test_backspace_at_start_is_noop() {
        let mut doc = Document::new();
        doc.insert_text("hello");
        doc.cursor.position = Position::new(0, 0);
        doc.backspace();
        assert_eq!(doc.buffer.to_string(), "hello");
    }

    // ── delete forward at end of buffer ──────────────────────────────

    #[test]
    fn test_delete_forward_at_end_is_noop() {
        let mut doc = Document::new();
        doc.insert_text("hello");
        // cursor is at end after insert
        doc.delete_forward();
        assert_eq!(doc.buffer.to_string(), "hello");
    }

    // ── backspace with selection ──────────────────────────────────────

    #[test]
    fn test_backspace_with_selection_deletes_selection() {
        let mut doc = Document::new();
        doc.insert_text("hello world");
        doc.cursor.position = Position::new(0, 5);
        doc.cursor.selection_anchor = Some(Position::new(0, 0));
        doc.backspace();
        assert_eq!(doc.buffer.to_string(), " world");
    }

    // ── delete_selection with empty selection ─────────────────────────

    #[test]
    fn test_delete_selection_empty_is_noop() {
        let mut doc = Document::new();
        doc.insert_text("hello");
        doc.cursor.position = Position::new(0, 3);
        doc.cursor.selection_anchor = Some(Position::new(0, 3));
        doc.delete_selection();
        assert_eq!(doc.buffer.to_string(), "hello");
        assert!(doc.cursor.selection_anchor.is_none()); // cleared
    }

    // ── selected_text with no selection ──────────────────────────────

    #[test]
    fn test_selected_text_none_when_no_selection() {
        let doc = Document::new();
        assert!(doc.selected_text().is_none());
    }

    // ── Default trait ────────────────────────────────────────────────

    #[test]
    fn test_document_default() {
        let doc = Document::default();
        assert!(doc.buffer.is_empty());
        assert_eq!(doc.title, "Untitled");
        assert!(!doc.modified);
        assert!(doc.session_id.is_none());
        assert!(doc.file_path.is_none());
    }

    // ── ScrollbarDrag enum ───────────────────────────────────────────

    #[test]
    fn test_scrollbar_drag_default() {
        let drag = ScrollbarDrag::default();
        assert_eq!(drag, ScrollbarDrag::None);
    }

    // ── Line change sync ─────────────────────────────────────────────

    #[test]
    fn test_line_changes_sync_on_delete_line() {
        let mut doc = Document::new();
        doc.insert_text("line1\nline2\nline3");
        // insert_text only calls mark_lines_modified, not sync_line_changes.
        // delete_line DOES call sync_line_changes which resizes the vector.
        doc.cursor.position = Position::new(1, 0);
        doc.delete_line();
        // After deletion: 2 lines remain, line_changes synced to 2
        assert_eq!(doc.buffer.len_lines(), 2);
        assert_eq!(doc.line_changes.len(), 2);
    }

    #[test]
    fn test_line_changes_sync_on_backspace_newline() {
        let mut doc = Document::new();
        doc.insert_text("a\nb");
        // Backspace at start of line 1 to merge lines
        doc.cursor.position = Position::new(1, 0);
        doc.backspace();
        // Buffer is now "ab" (1 line), sync_line_changes adjusts
        assert_eq!(doc.buffer.to_string(), "ab");
        assert_eq!(doc.line_changes.len(), 1);
    }

    // ── Multi-cursor: delete_forward_multi ────────────────────────────

    #[test]
    fn test_delete_forward_multi() {
        let mut doc = Document::new();
        doc.insert_text("Xhello\nXworld");
        doc.cursor.position = Position::new(0, 0);
        let mut sc = Cursor::new();
        sc.position = Position::new(1, 0);
        doc.secondary_cursors.push(sc);

        doc.delete_forward_multi();
        assert_eq!(doc.buffer.to_string(), "hello\nworld");
    }

    // ── Multi-cursor: insert_newline_multi ────────────────────────────

    #[test]
    fn test_insert_newline_multi() {
        let mut doc = Document::new();
        doc.insert_text("ab\ncd");
        doc.cursor.position = Position::new(0, 1);
        let mut sc = Cursor::new();
        sc.position = Position::new(1, 1);
        doc.secondary_cursors.push(sc);

        doc.insert_newline_multi();
        assert_eq!(doc.buffer.to_string(), "a\nb\nc\nd");
    }

    // ── Open file with encoding detection ────────────────────────────

    #[test]
    fn test_open_detects_encoding_and_line_ending() {
        let dir = std::env::temp_dir().join("rust_pad_test_enc");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("crlf_file.txt");
        std::fs::write(&path, "hello\r\nworld").unwrap();

        let doc = Document::open(&path).unwrap();
        assert_eq!(doc.line_ending, crate::encoding::LineEnding::CrLf);
        // Buffer content should be normalized to LF
        assert_eq!(doc.buffer.to_string(), "hello\nworld");
        assert_eq!(doc.title, "crlf_file.txt");
        assert!(!doc.modified);

        std::fs::remove_dir_all(&dir).ok();
    }

    // ── Undo snapshot tests ─────────────────────────────────────────

    #[test]
    fn test_snapshot_undo_records_and_undoes() {
        let mut doc = Document::new();
        doc.insert_text("banana\napple\ncherry");
        doc.history.force_group_break();

        let snapshot = doc.snapshot_for_undo();

        // Simulate a sort by replacing the buffer
        let _ = doc.buffer.remove(0, doc.buffer.len_chars());
        let _ = doc.buffer.insert(0, "apple\nbanana\ncherry");
        doc.record_undo_from_snapshot(snapshot);

        assert_eq!(doc.buffer.to_string(), "apple\nbanana\ncherry");

        doc.undo();
        assert_eq!(doc.buffer.to_string(), "banana\napple\ncherry");
    }

    #[test]
    fn test_snapshot_undo_redo_roundtrip() {
        let mut doc = Document::new();
        doc.insert_text("c\nb\na");
        doc.history.force_group_break();

        let snapshot = doc.snapshot_for_undo();
        let _ = doc.buffer.remove(0, doc.buffer.len_chars());
        let _ = doc.buffer.insert(0, "a\nb\nc");
        doc.record_undo_from_snapshot(snapshot);

        doc.undo();
        assert_eq!(doc.buffer.to_string(), "c\nb\na");

        doc.redo();
        assert_eq!(doc.buffer.to_string(), "a\nb\nc");
    }

    #[test]
    fn test_snapshot_noop_when_unchanged() {
        let mut doc = Document::new();
        doc.insert_text("hello");
        doc.history.force_group_break();

        let snapshot = doc.snapshot_for_undo();
        // No changes to buffer
        doc.record_undo_from_snapshot(snapshot);

        // Undo should undo the insert, not a no-op snapshot
        doc.undo();
        assert_eq!(doc.buffer.to_string(), "");
    }

    #[test]
    fn test_snapshot_restores_cursor() {
        let mut doc = Document::new();
        doc.insert_text("line2\nline1");
        doc.cursor.position = Position::new(0, 0);
        doc.history.force_group_break();

        let snapshot = doc.snapshot_for_undo();
        let _ = doc.buffer.remove(0, doc.buffer.len_chars());
        let _ = doc.buffer.insert(0, "line1\nline2");
        doc.cursor.position = Position::new(1, 5);
        doc.record_undo_from_snapshot(snapshot);

        doc.undo();
        assert_eq!(doc.cursor.position, Position::new(0, 0));
    }

    // ── reload_from_disk tests ──────────────────────────────────────

    #[test]
    fn test_reload_from_disk_updates_content() {
        let dir = std::env::temp_dir().join("rust_pad_test_reload");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("reload_test.txt");
        std::fs::write(&path, "original content").unwrap();

        let mut doc = Document::open(&path).unwrap();
        assert_eq!(doc.buffer.to_string(), "original content");

        // Modify file externally
        std::fs::write(&path, "updated content").unwrap();
        doc.reload_from_disk().unwrap();
        assert_eq!(doc.buffer.to_string(), "updated content");
        assert!(!doc.modified);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_reload_from_disk_updates_mtime() {
        let dir = std::env::temp_dir().join("rust_pad_test_reload_mtime");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("mtime_test.txt");
        std::fs::write(&path, "initial").unwrap();

        let mut doc = Document::open(&path).unwrap();
        let mtime_after_open = doc.last_known_mtime;
        assert!(mtime_after_open.is_some());

        // Wait briefly, then modify
        std::thread::sleep(std::time::Duration::from_millis(50));
        std::fs::write(&path, "changed").unwrap();

        doc.reload_from_disk().unwrap();
        assert!(doc.last_known_mtime.is_some());
        // mtime should be at least as recent as the open mtime
        assert!(doc.last_known_mtime >= mtime_after_open);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_reload_from_disk_without_path_errors() {
        let mut doc = Document::new();
        let result = doc.reload_from_disk();
        assert!(result.is_err());
    }

    #[test]
    fn test_reload_from_disk_preserves_live_monitoring() {
        let dir = std::env::temp_dir().join("rust_pad_test_reload_live");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("live_test.txt");
        std::fs::write(&path, "content").unwrap();

        let mut doc = Document::open(&path).unwrap();
        doc.live_monitoring = true;
        doc.reload_from_disk().unwrap();
        assert!(
            doc.live_monitoring,
            "reload should preserve live_monitoring flag"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_reload_from_disk_detects_encoding_change() {
        let dir = std::env::temp_dir().join("rust_pad_test_reload_enc");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("encoding_test.txt");
        std::fs::write(&path, "hello").unwrap();

        let mut doc = Document::open(&path).unwrap();
        assert_eq!(doc.buffer.to_string(), "hello");

        // Write UTF-8 BOM content
        let mut bom_content = vec![0xEF, 0xBB, 0xBF];
        bom_content.extend_from_slice(b"world");
        std::fs::write(&path, &bom_content).unwrap();

        doc.reload_from_disk().unwrap();
        assert_eq!(doc.buffer.to_string(), "world");

        std::fs::remove_dir_all(&dir).ok();
    }

    // ── save sets last_saved_at and last_known_mtime ────────────────

    #[test]
    fn test_save_sets_last_saved_at() {
        let dir = std::env::temp_dir().join("rust_pad_test_save_time");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("save_time_test.txt");

        let mut doc = Document::new();
        doc.insert_text("content");
        assert!(doc.last_saved_at.is_none());

        doc.save_to(&path).unwrap();
        assert!(doc.last_saved_at.is_some());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_save_sets_last_known_mtime() {
        let dir = std::env::temp_dir().join("rust_pad_test_save_mtime");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("save_mtime_test.txt");

        let mut doc = Document::new();
        doc.insert_text("content");
        assert!(doc.last_known_mtime.is_none());

        doc.save_to(&path).unwrap();
        assert!(doc.last_known_mtime.is_some());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_open_sets_last_known_mtime() {
        let dir = std::env::temp_dir().join("rust_pad_test_open_mtime");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("open_mtime_test.txt");
        std::fs::write(&path, "content").unwrap();

        let doc = Document::open(&path).unwrap();
        assert!(doc.last_known_mtime.is_some());
        assert!(
            doc.last_saved_at.is_none(),
            "freshly opened doc should have no saved_at"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    // ── live_monitoring and new document defaults ────────────────────

    #[test]
    fn test_new_document_monitoring_defaults() {
        let doc = Document::new();
        assert!(!doc.live_monitoring);
        assert!(doc.last_known_mtime.is_none());
        assert!(doc.last_saved_at.is_none());
    }

    #[test]
    fn test_opened_document_monitoring_defaults() {
        let dir = std::env::temp_dir().join("rust_pad_test_open_live");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("live_default.txt");
        std::fs::write(&path, "content").unwrap();

        let doc = Document::open(&path).unwrap();
        assert!(
            !doc.live_monitoring,
            "live_monitoring should default to false on open"
        );

        std::fs::remove_dir_all(&dir).ok();
    }
}
