//! Integration of the per-file view-state persistence with the App.
//!
//! Saves cursor + scroll position when a file-backed tab is closed or
//! the app exits, and restores them when a file is opened. Keying is
//! done by canonical path string; see `rust_pad_config::paths::canonical_path_key`.

use std::path::Path;

use rust_pad_config::{paths, ViewState, ViewStateStore};
use rust_pad_core::document::Document;

use super::App;

impl App {
    /// Opens (or creates) the per-file view-state database.
    ///
    /// Returns `None` if the store cannot be opened. A failed open is
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
    /// session has been restored. `try_open_file_from_bytes` covers
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

    /// Persists cursor + scroll for every file-backed open tab.
    ///
    /// [`Self::persist_view_state`] no-ops for untitled tabs and for a missing
    /// store, so this is safe to call unconditionally. Called from the periodic
    /// flush and from `on_exit`, so an open file's scroll and cursor are captured
    /// during the session as well as at exit.
    pub(crate) fn persist_open_view_states(&self) {
        for doc in &self.tabs.documents {
            self.persist_view_state(doc);
        }
    }

    /// Fingerprint of the persisted view-state across all file-backed tabs.
    ///
    /// Hashes each file-backed document's path, scroll offsets, and cursor
    /// position so the flush tick can skip a redb write when nothing moved.
    /// The `f32` offsets are hashed by bit pattern (they are non-negative and
    /// never `NaN`; scroll math clamps against `0.0`). Untitled tabs carry no
    /// persisted view-state and are excluded.
    pub(crate) fn view_state_sig(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        for doc in &self.tabs.documents {
            let Some(path) = doc.file_path.as_ref() else {
                continue;
            };
            path.hash(&mut h);
            doc.scroll_y.to_bits().hash(&mut h);
            doc.scroll_x.to_bits().hash(&mut h);
            doc.cursor.position.line.hash(&mut h);
            doc.cursor.position.col.hash(&mut h);
        }
        h.finish()
    }

    /// Periodic entry point: persists view-state for open file-backed tabs, but
    /// only when it changed since the last persist. No-op when the view-state
    /// store is unavailable.
    ///
    /// Mirrors `maybe_autosave_session`, giving scroll + cursor the same
    /// crash-safe cadence as the session's tab list: both are written on the
    /// flush tick, so an unexpected termination keeps the on-screen position.
    pub(crate) fn maybe_persist_view_states(&mut self) {
        if self.view_state_store.is_none() {
            return;
        }
        let sig = self.view_state_sig();
        if self.last_view_state_sig == Some(sig) {
            tracing::trace!("view-state autosave skipped: unchanged");
            return;
        }
        self.persist_open_view_states();
        self.last_view_state_sig = Some(sig);
        tracing::debug!(
            count = self
                .tabs
                .documents
                .iter()
                .filter(|d| d.file_path.is_some())
                .count(),
            "view-state autosave persisted"
        );
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

    /// Writes a file with `n` short lines into `dir` and returns its path.
    fn file_with_lines(dir: &TempDir, name: &str, n: usize) -> std::path::PathBuf {
        let path = dir.path().join(name);
        let body: String = (0..n).map(|i| format!("line {i}\n")).collect();
        std::fs::write(&path, body).expect("write file");
        path
    }

    #[test]
    fn maybe_persist_writes_view_state_without_on_exit() {
        // Simulates a crash: the periodic flush persisted view-state, but
        // on_exit never ran. The record must still be on disk.
        let (store, _sdir) = store_in_tempdir();
        let mut app = test_app();
        app.view_state_store = Some(store);

        let file_dir = TempDir::new().expect("file dir");
        let file = file_with_lines(&file_dir, "crash.txt", 20);
        app.tabs.open_file(&file).expect("open");
        let active = app.tabs.active;
        app.tabs.documents[active].scroll_y = 7.0;

        app.maybe_persist_view_states();

        let key = paths::canonical_path_key(&file);
        let loaded = app
            .view_state_store
            .as_ref()
            .unwrap()
            .load(&key)
            .expect("load")
            .expect("record present after periodic persist");
        assert_eq!(loaded.scroll_y, 7.0);
    }

    #[test]
    fn maybe_persist_full_crash_roundtrip_restores_scroll() {
        use rust_pad_config::session::SessionStore;

        // Explicit shared store paths so a second App can reopen them after the
        // first is dropped (redb holds an exclusive file lock).
        let store_dir = TempDir::new().expect("store dir");
        let session_path = store_dir.path().join("session.redb");
        let vs_path = store_dir.path().join("view-state.redb");

        let file_dir = TempDir::new().expect("file dir");
        let file = file_with_lines(&file_dir, "roundtrip.txt", 40);

        // Session 1: open, scroll, persist session + view-state, then drop
        // WITHOUT on_exit (as if the process was killed).
        {
            let mut app = test_app();
            app.session_store = Some(SessionStore::open(&session_path).expect("session store"));
            app.view_state_store = Some(ViewStateStore::open(&vs_path).expect("vs store"));
            app.tabs.open_file(&file).expect("open");
            let active = app.tabs.active;
            app.tabs.documents[active].scroll_y = 12.0;
            app.tabs.documents[active].cursor.position = rust_pad_core::cursor::Position::new(5, 2);

            app.run_session_snapshot(false);
            app.maybe_persist_view_states();
        }

        // Session 2: reopen the same stores and restore.
        let mut app2 = test_app();
        app2.session_store = Some(SessionStore::open(&session_path).expect("session store 2"));
        app2.view_state_store = Some(ViewStateStore::open(&vs_path).expect("vs store 2"));
        App::restore_session(
            &mut app2.tabs,
            &app2.session_store,
            app2.session_content_max_kb,
        );
        app2.restore_view_states_for_open_files();

        let restored = app2
            .tabs
            .documents
            .iter()
            .find(|d| d.file_path.is_some())
            .expect("file tab restored");
        assert_eq!(restored.scroll_y, 12.0);
        assert_eq!(restored.cursor.position.line, 5);
        assert_eq!(restored.cursor.position.col, 2);
    }

    #[test]
    fn maybe_persist_skips_redundant_write_when_unchanged() {
        let (store, _sdir) = store_in_tempdir();
        let mut app = test_app();
        app.view_state_store = Some(store);

        let file_dir = TempDir::new().expect("file dir");
        let file = file_with_lines(&file_dir, "gate.txt", 20);
        app.tabs.open_file(&file).expect("open");
        app.tabs.active_doc_mut().scroll_y = 3.0;

        app.maybe_persist_view_states();

        // Overwrite the record out-of-band with a sentinel value.
        let key = paths::canonical_path_key(&file);
        let sentinel = ViewState {
            scroll_y: 99.0,
            scroll_x: 0.0,
            cursor_line: 0,
            cursor_col: 0,
            last_used_unix_ms: 0,
        };
        app.view_state_store
            .as_ref()
            .unwrap()
            .save(&key, &sentinel)
            .expect("save sentinel");

        // The view didn't move, so the gate must skip and leave the sentinel.
        app.maybe_persist_view_states();

        let loaded = app
            .view_state_store
            .as_ref()
            .unwrap()
            .load(&key)
            .expect("load")
            .expect("some");
        assert_eq!(
            loaded.scroll_y, 99.0,
            "unchanged view must not trigger a redb write"
        );
    }

    #[test]
    fn maybe_persist_rewrites_after_scroll_change() {
        let (store, _sdir) = store_in_tempdir();
        let mut app = test_app();
        app.view_state_store = Some(store);

        let file_dir = TempDir::new().expect("file dir");
        let file = file_with_lines(&file_dir, "move.txt", 20);
        app.tabs.open_file(&file).expect("open");
        app.tabs.active_doc_mut().scroll_y = 1.0;

        let sig1 = app.view_state_sig();
        app.maybe_persist_view_states();

        app.tabs.active_doc_mut().scroll_y = 6.0;
        let sig2 = app.view_state_sig();
        assert_ne!(sig1, sig2, "moving the scroll must change the fingerprint");

        app.maybe_persist_view_states();
        let key = paths::canonical_path_key(&file);
        let loaded = app
            .view_state_store
            .as_ref()
            .unwrap()
            .load(&key)
            .expect("load")
            .expect("some");
        assert_eq!(loaded.scroll_y, 6.0);
    }

    #[test]
    fn maybe_persist_noop_without_store() {
        let mut app = test_app();
        assert!(app.view_state_store.is_none());
        app.maybe_persist_view_states();
        assert!(app.last_view_state_sig.is_none());
    }

    #[test]
    fn on_exit_persists_view_state() {
        use eframe::App as _;

        let (store, _sdir) = store_in_tempdir();
        let mut app = test_app();
        app.view_state_store = Some(store);
        let cfg_dir = TempDir::new().expect("cfg dir");
        app.config_path = cfg_dir.path().join("rust-pad.json");

        let file_dir = TempDir::new().expect("file dir");
        let file = file_with_lines(&file_dir, "exit.txt", 20);
        app.tabs.open_file(&file).expect("open");
        app.tabs.active_doc_mut().scroll_y = 9.0;

        app.on_exit();

        let key = paths::canonical_path_key(&file);
        let loaded = app
            .view_state_store
            .as_ref()
            .unwrap()
            .load(&key)
            .expect("load")
            .expect("some");
        assert_eq!(loaded.scroll_y, 9.0);
    }
}
