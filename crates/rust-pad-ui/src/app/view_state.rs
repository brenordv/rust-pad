//! Integration of the per-file view-state persistence with the App.
//!
//! Saves cursor + scroll position when a file-backed tab is closed or
//! the app exits, and restores them when a file is opened. Keying is
//! done by canonical path string — see `rust_pad_config::paths::canonical_path_key`.

use std::path::Path;

use rust_pad_config::{paths, ViewState, ViewStateStore};
use rust_pad_core::document::Document;

use super::App;

impl App {
    /// Opens (or creates) the per-file view-state database.
    ///
    /// Returns `None` if the store cannot be opened — a failed open is
    /// non-fatal; the app still works, restored cursor/scroll just won't
    /// persist across sessions.
    pub(crate) fn init_view_state_store(portable: bool) -> Option<ViewStateStore> {
        let path = if portable {
            paths::portable_view_state_file_path()
        } else {
            ViewStateStore::view_state_path()
        };
        match ViewStateStore::open(&path) {
            Ok(store) => Some(store),
            Err(e) => {
                tracing::warn!("Failed to open view-state store: {e}");
                None
            }
        }
    }

    /// Applies persisted view-state (cursor + scroll) to every currently
    /// open file-backed document. Called once during `App::new` after the
    /// session has been restored — `try_open_file_from_bytes` covers
    /// runtime opens, but session restore opens files synchronously via
    /// `tabs.open_file`, bypassing that hook.
    ///
    /// Uses disjoint field borrows (`self.view_state_store` and
    /// `self.tabs.documents` are distinct App fields) so a single `&mut self`
    /// receiver is sound.
    pub(crate) fn restore_view_states_for_open_files(&mut self) {
        let store = self.view_state_store.as_ref();
        for doc in &mut self.tabs.documents {
            let Some(path) = doc.file_path.clone() else {
                continue;
            };
            apply_saved_view_state(store, doc, &path);
        }
    }

    /// Captures `doc`'s current scroll + cursor and persists it under
    /// the file's canonical path. No-op when the tab is not file-backed.
    pub(crate) fn persist_view_state(&self, doc: &Document) {
        let (Some(path), Some(store)) = (doc.file_path.as_ref(), self.view_state_store.as_ref())
        else {
            return;
        };
        let key = paths::canonical_path_key(path);
        let state = ViewState {
            scroll_y: doc.scroll_y,
            scroll_x: doc.scroll_x,
            cursor_line: doc.cursor.position.line,
            cursor_col: doc.cursor.position.col,
            last_used_unix_ms: chrono::Utc::now().timestamp_millis(),
        };
        if let Err(e) = store.save(&key, &state) {
            tracing::warn!("Failed to save view-state for '{}': {e}", path.display());
        }
    }
}

/// Looks up the saved view-state for `path` and applies it to `doc`.
///
/// Security hardening: explicitly clamps `cursor_line` and `cursor_col`
/// against the loaded buffer so a tampered or stale record cannot place
/// the cursor outside the document.
///
/// Free function (rather than method on App) so the caller can hold a
/// `&mut Document` borrowed out of `self.tabs.documents` without
/// conflicting with a `&self` whole-self borrow.
pub(crate) fn apply_saved_view_state(
    store: Option<&ViewStateStore>,
    doc: &mut Document,
    path: &Path,
) {
    let Some(store) = store else {
        return;
    };
    let key = paths::canonical_path_key(path);
    let state = match store.load(&key) {
        Ok(Some(s)) => s,
        Ok(None) => return,
        Err(e) => {
            tracing::warn!("Failed to load view-state for '{}': {e}", path.display());
            return;
        }
    };

    let line_count = doc.buffer.len_lines();
    let max_line = line_count.saturating_sub(1);
    let cursor_line = state.cursor_line.min(max_line);
    let line_len = doc.buffer.line_len_chars(cursor_line).unwrap_or(0);
    let cursor_col = state.cursor_col.min(line_len);

    doc.cursor.position = rust_pad_core::cursor::Position::new(cursor_line, cursor_col);
    doc.scroll_y = state.scroll_y.max(0.0);
    doc.scroll_x = state.scroll_x.max(0.0);

    tracing::debug!(
        "Restored view-state for '{}': line={cursor_line} col={cursor_col} scroll_y={scroll_y}",
        path.display(),
        scroll_y = doc.scroll_y,
    );
}

#[cfg(test)]
mod tests {
    use super::super::tests::test_app;
    use super::*;
    use rust_pad_core::document::Document;
    use tempfile::TempDir;

    fn store_in_tempdir() -> (ViewStateStore, TempDir) {
        let dir = TempDir::new().expect("temp dir");
        let path = dir.path().join("vs.redb");
        (
            ViewStateStore::open(&path).expect("open view-state store"),
            dir,
        )
    }

    #[test]
    fn persist_view_state_noop_when_path_missing() {
        let app = test_app();
        let doc = Document::new();
        // No path on doc → no-op even if a store were present.
        app.persist_view_state(&doc);
    }

    #[test]
    fn persist_then_restore_roundtrip() {
        let (store, _dir) = store_in_tempdir();
        let mut app = test_app();
        app.view_state_store = Some(store);

        let file_dir = TempDir::new().expect("file dir");
        let file = file_dir.path().join("sample.txt");
        std::fs::write(&file, "line0\nline1\nline2\nline3\n").expect("write");

        let mut doc = Document::open(&file).expect("open");
        doc.cursor.position = rust_pad_core::cursor::Position::new(2, 3);
        doc.scroll_y = 1.0;

        app.persist_view_state(&doc);

        // Build a fresh document from disk and restore.
        let mut fresh = Document::open(&file).expect("open 2");
        assert_eq!(fresh.cursor.position.line, 0);
        assert_eq!(fresh.scroll_y, 0.0);

        apply_saved_view_state(app.view_state_store.as_ref(), &mut fresh, &file);
        assert_eq!(fresh.cursor.position.line, 2);
        assert_eq!(fresh.cursor.position.col, 3);
        assert_eq!(fresh.scroll_y, 1.0);
    }

    #[test]
    fn restore_clamps_cursor_to_buffer_after_truncation() {
        let (store, _dir) = store_in_tempdir();
        let mut app = test_app();
        app.view_state_store = Some(store);

        let file_dir = TempDir::new().expect("file dir");
        let file = file_dir.path().join("shrink.txt");
        std::fs::write(&file, "a\nb\nc\nd\ne\n").expect("write");

        let mut doc = Document::open(&file).expect("open");
        doc.cursor.position = rust_pad_core::cursor::Position::new(4, 1);
        app.persist_view_state(&doc);

        // Truncate the file: now only one line.
        std::fs::write(&file, "x\n").expect("rewrite");
        let mut fresh = Document::open(&file).expect("open 2");
        apply_saved_view_state(app.view_state_store.as_ref(), &mut fresh, &file);

        // Cursor line is clamped to within the new buffer.
        assert!(fresh.cursor.position.line < fresh.buffer.len_lines());
    }

    #[test]
    fn restore_noop_when_no_store() {
        let mut doc = Document::new();
        apply_saved_view_state(None, &mut doc, Path::new("/anything"));
        assert_eq!(doc.cursor.position.line, 0);
    }

    #[test]
    fn restore_view_states_for_open_files_applies_saved_state() {
        // Simulates the App::new startup path: open a file, persist its
        // view-state, then drop and reopen the file via the same store and
        // verify the helper restores cursor + scroll.
        let (store, _dir) = store_in_tempdir();
        let mut app = test_app();
        app.view_state_store = Some(store);

        let file_dir = TempDir::new().expect("file dir");
        let file = file_dir.path().join("startup.txt");
        std::fs::write(&file, "a\nb\nc\nd\ne\n").expect("write");

        // Open the file into the test App and persist a non-default state.
        app.tabs.open_file(&file).expect("open");
        let active = app.tabs.active;
        app.tabs.documents[active].cursor.position = rust_pad_core::cursor::Position::new(3, 1);
        app.tabs.documents[active].scroll_y = 2.5;
        app.persist_view_state(&app.tabs.documents[active]);

        // Simulate a fresh session: reload the document from disk to
        // erase in-memory cursor/scroll, then call the startup restore.
        let fresh_doc = Document::open(&file).expect("reload");
        app.tabs.documents[active] = fresh_doc;
        assert_eq!(app.tabs.documents[active].cursor.position.line, 0);
        assert_eq!(app.tabs.documents[active].scroll_y, 0.0);

        app.restore_view_states_for_open_files();

        assert_eq!(app.tabs.documents[active].cursor.position.line, 3);
        assert_eq!(app.tabs.documents[active].cursor.position.col, 1);
        assert_eq!(app.tabs.documents[active].scroll_y, 2.5);
    }

    #[test]
    fn restore_view_states_for_open_files_skips_untitled_docs() {
        // Untitled (no file_path) docs must be left untouched.
        let (store, _dir) = store_in_tempdir();
        let mut app = test_app();
        app.view_state_store = Some(store);

        let active = app.tabs.active;
        assert!(app.tabs.documents[active].file_path.is_none());
        app.tabs.documents[active].scroll_y = 10.0;

        app.restore_view_states_for_open_files();

        // Untitled doc preserved as-is.
        assert_eq!(app.tabs.documents[active].scroll_y, 10.0);
    }
}
