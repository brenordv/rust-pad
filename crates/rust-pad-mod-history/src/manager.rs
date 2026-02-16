/// Core undo/redo manager with tiered storage.
///
/// Recent edits are kept in memory (hot cache). When the hot cache exceeds
/// capacity, older groups are spilled to disk. On undo past the hot cache,
/// groups are loaded from disk transparently.
use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};

use crate::config::HistoryConfig;
use crate::operation::{EditGroup, EditOperation};
use crate::persistence::PersistenceLayer;

/// Manages undo/redo history for a single document.
///
/// Each document gets its own `UndoManager` with an independent history stack.
/// The manager can optionally persist history to disk via a shared
/// `PersistenceLayer`.
pub struct UndoManager {
    /// In-memory undo stack, ordered by seq ascending (oldest first).
    hot_undo: Vec<EditGroup>,
    /// In-memory redo stack, ordered with most-recently-undone on top.
    redo_stack: Vec<EditGroup>,
    /// Next sequence number to assign to new groups.
    next_seq: u64,
    /// Document identifier used as the persistence key.
    doc_id: String,
    /// Whether recording is active (set to false during undo/redo replay).
    recording: bool,
    /// Timestamp of the last recorded edit, used for grouping.
    last_edit_time: Option<Instant>,
    /// Configuration parameters.
    config: HistoryConfig,
    /// Optional disk persistence (None = in-memory only).
    persistence: Option<Arc<PersistenceLayer>>,
    /// Whether in-memory state has changed since the last flush.
    dirty: bool,
    /// Number of groups on disk that are NOT in the hot cache.
    cold_count: usize,
}

impl std::fmt::Debug for UndoManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UndoManager")
            .field("doc_id", &self.doc_id)
            .field("hot_undo_len", &self.hot_undo.len())
            .field("redo_len", &self.redo_stack.len())
            .field("next_seq", &self.next_seq)
            .field("recording", &self.recording)
            .field("dirty", &self.dirty)
            .field("cold_count", &self.cold_count)
            .finish()
    }
}

impl UndoManager {
    /// Creates a new empty UndoManager.
    ///
    /// Pass `persistence: None` for in-memory-only mode (useful in tests
    /// or for documents that don't need disk persistence).
    pub fn new(
        doc_id: String,
        config: HistoryConfig,
        persistence: Option<Arc<PersistenceLayer>>,
    ) -> Self {
        Self {
            hot_undo: Vec::new(),
            redo_stack: Vec::new(),
            next_seq: 0,
            doc_id,
            recording: true,
            last_edit_time: None,
            config,
            persistence,
            dirty: false,
            cold_count: 0,
        }
    }

    /// Creates an in-memory-only UndoManager with default config.
    ///
    /// Convenience constructor for tests and simple usage.
    pub fn in_memory() -> Self {
        Self::new(String::from("test"), HistoryConfig::default(), None)
    }

    /// Loads existing history from disk, or creates a fresh manager.
    ///
    /// Reads stored edit groups into the hot cache and restores the
    /// sequence counter. If no history exists on disk, behaves like `new()`.
    ///
    /// # Errors
    ///
    /// Returns an error if the persistence layer fails to read.
    pub fn load_or_new(
        doc_id: String,
        config: HistoryConfig,
        persistence: Option<Arc<PersistenceLayer>>,
    ) -> Result<Self> {
        let (hot_undo, next_seq, cold_count) = match &persistence {
            Some(pl) => {
                let stored_seq = pl
                    .load_meta(&doc_id)
                    .context("Failed to load document metadata")?;

                match stored_seq {
                    Some(seq) => {
                        let all_groups = pl
                            .read_groups(&doc_id)
                            .context("Failed to load history from disk")?;
                        let total = all_groups.len();
                        let skip = total.saturating_sub(config.hot_capacity);
                        let hot: Vec<EditGroup> = all_groups.into_iter().skip(skip).collect();
                        let cold = total.saturating_sub(hot.len());
                        (hot, seq, cold)
                    }
                    None => (Vec::new(), 0, 0),
                }
            }
            None => (Vec::new(), 0, 0),
        };

        Ok(Self {
            hot_undo,
            redo_stack: Vec::new(),
            next_seq,
            doc_id,
            recording: true,
            last_edit_time: None,
            config,
            persistence,
            dirty: false,
            cold_count,
        })
    }

    /// Returns the document ID.
    pub fn doc_id(&self) -> &str {
        &self.doc_id
    }

    /// Records an edit operation.
    ///
    /// Groups with the previous operation if within the grouping timeout.
    /// Clears the redo stack when a new edit is recorded.
    pub fn record(&mut self, op: EditOperation) {
        if !self.recording {
            return;
        }

        let now = Instant::now();
        let timeout = Duration::from_millis(self.config.group_timeout_ms);

        // Try to group with the last group if within timeout
        if let Some(last_group) = self.hot_undo.last_mut() {
            if let Some(last_time) = self.last_edit_time {
                if now.duration_since(last_time) < timeout {
                    last_group.operations.push(op);
                    self.last_edit_time = Some(now);
                    self.redo_stack.clear();
                    self.dirty = true;
                    return;
                }
            }
        }

        // Create a new group
        let group = EditGroup {
            operations: vec![op],
            seq: self.next_seq,
        };
        self.next_seq += 1;
        self.hot_undo.push(group);
        self.last_edit_time = Some(now);
        self.redo_stack.clear();
        self.dirty = true;

        // Enforce capacity limits
        if self.persistence.is_some() {
            if self.hot_undo.len() > self.config.hot_capacity {
                if let Err(e) = self.maybe_spill() {
                    tracing::warn!("Failed to spill history to disk: {e}");
                }
            }
        } else if self.hot_undo.len() > self.config.max_history_depth {
            let excess = self.hot_undo.len() - self.config.max_history_depth;
            self.hot_undo.drain(..excess);
        }
    }

    /// Forces a group break so the next edit starts a new undo group.
    pub fn force_group_break(&mut self) {
        self.last_edit_time = None;
    }

    /// Undoes the most recent group.
    ///
    /// Returns the operations that should be applied in reverse order
    /// to undo the edit. Returns `None` if there's nothing to undo.
    pub fn undo(&mut self) -> Option<Vec<EditOperation>> {
        // If hot cache is empty, try loading from cold storage
        if self.hot_undo.is_empty() {
            if let Err(e) = self.load_from_cold() {
                tracing::warn!("Failed to load history from disk: {e}");
                return None;
            }
        }

        let group = self.hot_undo.pop()?;
        let ops = group.operations.clone();
        self.redo_stack.push(group);
        self.dirty = true;
        Some(ops)
    }

    /// Redoes the most recently undone group.
    ///
    /// Returns the operations that should be applied in forward order.
    /// Returns `None` if there's nothing to redo.
    pub fn redo(&mut self) -> Option<Vec<EditOperation>> {
        let group = self.redo_stack.pop()?;
        let ops = group.operations.clone();
        self.hot_undo.push(group);
        self.dirty = true;
        Some(ops)
    }

    /// Whether undo is available (in memory or on disk).
    pub fn can_undo(&self) -> bool {
        !self.hot_undo.is_empty() || self.cold_count > 0
    }

    /// Whether redo is available.
    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    /// Temporarily disables recording (used during undo/redo replay).
    pub fn pause_recording(&mut self) {
        self.recording = false;
    }

    /// Re-enables recording after a pause.
    pub fn resume_recording(&mut self) {
        self.recording = true;
    }

    /// Clears all history from memory and disk.
    ///
    /// # Errors
    ///
    /// Returns an error if disk cleanup fails.
    pub fn clear(&mut self) -> Result<()> {
        self.hot_undo.clear();
        self.redo_stack.clear();
        self.next_seq = 0;
        self.last_edit_time = None;
        self.dirty = false;
        self.cold_count = 0;

        if let Some(pl) = &self.persistence {
            pl.delete_document(&self.doc_id)
                .context("Failed to clear history from disk")?;
        }
        Ok(())
    }

    /// Flushes in-memory history to disk.
    ///
    /// Called periodically, on document save, and on application shutdown.
    /// No-op if the manager is in-memory-only or nothing has changed.
    ///
    /// # Errors
    ///
    /// Returns an error if the disk write fails.
    pub fn flush(&mut self) -> Result<()> {
        if !self.dirty {
            return Ok(());
        }

        if let Some(pl) = &self.persistence {
            pl.write_groups(&self.doc_id, &self.hot_undo)
                .context("Failed to flush history to disk")?;
            pl.save_meta(&self.doc_id, self.next_seq)
                .context("Failed to save history metadata")?;
            self.dirty = false;
        }
        Ok(())
    }

    /// Deletes all persisted history for this document.
    ///
    /// Called when a tab is explicitly closed (cleanup policy D).
    ///
    /// # Errors
    ///
    /// Returns an error if disk cleanup fails.
    pub fn delete_history(&mut self) -> Result<()> {
        self.clear()
    }

    /// Spills the oldest half of the hot cache to disk.
    fn maybe_spill(&mut self) -> Result<()> {
        let Some(pl) = &self.persistence else {
            return Ok(());
        };

        let spill_count = self.hot_undo.len() / 2;
        if spill_count == 0 {
            return Ok(());
        }

        let to_spill: Vec<EditGroup> = self.hot_undo.drain(..spill_count).collect();
        pl.write_groups(&self.doc_id, &to_spill)
            .context("Failed to spill groups to disk")?;
        self.cold_count += spill_count;

        // Enforce max history depth by evicting oldest from cold.
        // Reserve room for hot_capacity items so that between spills
        // the total (cold + hot) never exceeds max_history_depth.
        let desired_cold = self
            .config
            .max_history_depth
            .saturating_sub(self.config.hot_capacity);
        if self.cold_count > desired_cold {
            let excess = self.cold_count - desired_cold;
            let evicted = pl
                .evict_oldest(&self.doc_id, excess)
                .context("Failed to evict old history")?;
            self.cold_count = self.cold_count.saturating_sub(evicted);
        }

        pl.save_meta(&self.doc_id, self.next_seq)
            .context("Failed to save metadata after spill")?;
        Ok(())
    }

    /// Loads groups from cold storage into the hot cache.
    ///
    /// Called when hot cache is empty and the user attempts to undo.
    fn load_from_cold(&mut self) -> Result<()> {
        if self.cold_count == 0 {
            return Ok(());
        }

        let Some(pl) = &self.persistence else {
            return Ok(());
        };

        let groups = pl
            .read_groups(&self.doc_id)
            .context("Failed to load groups from cold storage")?;

        // Filter out groups that are currently in the redo stack
        // (they were flushed to disk before being undone)
        let redo_seqs: HashSet<u64> = self.redo_stack.iter().map(|g| g.seq).collect();
        self.hot_undo = groups
            .into_iter()
            .filter(|g| !redo_seqs.contains(&g.seq))
            .collect();
        self.cold_count = 0;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::operation::CursorSnapshot;
    use tempfile::TempDir;

    fn make_op(pos: usize, inserted: &str, deleted: &str) -> EditOperation {
        EditOperation {
            position: pos,
            inserted: inserted.to_string(),
            deleted: deleted.to_string(),
            cursor_before: CursorSnapshot::default(),
            cursor_after: CursorSnapshot::default(),
        }
    }

    fn small_config() -> HistoryConfig {
        HistoryConfig {
            hot_capacity: 5,
            max_history_depth: 20,
            group_timeout_ms: 500,
            data_dir: std::path::PathBuf::from("."),
        }
    }

    fn persistent_manager(dir: &std::path::Path) -> (UndoManager, Arc<PersistenceLayer>) {
        let pl = PersistenceLayer::open(dir).expect("open db");
        let config = small_config();
        let mgr = UndoManager::new("test-doc".to_string(), config, Some(Arc::clone(&pl)));
        (mgr, pl)
    }

    // --- Basic undo/redo tests (in-memory) ---

    #[test]
    fn test_undo_redo_basic() {
        let mut mgr = UndoManager::in_memory();
        mgr.force_group_break();
        mgr.record(make_op(0, "a", ""));
        mgr.force_group_break();
        mgr.record(make_op(1, "b", ""));

        assert!(mgr.can_undo());
        let ops = mgr.undo().expect("undo");
        assert_eq!(ops.len(), 1);
        assert_eq!(ops[0].inserted, "b");

        assert!(mgr.can_redo());
        let ops = mgr.redo().expect("redo");
        assert_eq!(ops.len(), 1);
        assert_eq!(ops[0].inserted, "b");
    }

    #[test]
    fn test_redo_cleared_on_new_edit() {
        let mut mgr = UndoManager::in_memory();
        mgr.force_group_break();
        mgr.record(make_op(0, "a", ""));
        mgr.force_group_break();
        mgr.record(make_op(1, "b", ""));

        mgr.undo();
        assert!(mgr.can_redo());

        mgr.record(make_op(1, "c", ""));
        assert!(!mgr.can_redo());
    }

    #[test]
    fn test_empty_history() {
        let mut mgr = UndoManager::in_memory();
        assert!(!mgr.can_undo());
        assert!(!mgr.can_redo());
        assert!(mgr.undo().is_none());
        assert!(mgr.redo().is_none());
    }

    #[test]
    fn test_pause_recording() {
        let mut mgr = UndoManager::in_memory();
        mgr.pause_recording();
        mgr.record(make_op(0, "a", ""));
        assert!(!mgr.can_undo());

        mgr.resume_recording();
        mgr.record(make_op(0, "b", ""));
        assert!(mgr.can_undo());
    }

    #[test]
    fn test_force_group_break() {
        let mut mgr = UndoManager::in_memory();

        // Without break, edits within timeout are grouped
        mgr.record(make_op(0, "a", ""));
        mgr.record(make_op(1, "b", "")); // Grouped with "a"

        // One undo should remove both
        let ops = mgr.undo().expect("undo");
        assert_eq!(ops.len(), 2);
        assert!(!mgr.can_undo());
    }

    #[test]
    fn test_force_group_break_separates_groups() {
        let mut mgr = UndoManager::in_memory();

        mgr.record(make_op(0, "a", ""));
        mgr.force_group_break();
        mgr.record(make_op(1, "b", ""));

        // First undo gets "b" only
        let ops = mgr.undo().expect("undo");
        assert_eq!(ops.len(), 1);
        assert_eq!(ops[0].inserted, "b");

        // Second undo gets "a"
        assert!(mgr.can_undo());
        let ops = mgr.undo().expect("undo");
        assert_eq!(ops.len(), 1);
        assert_eq!(ops[0].inserted, "a");
    }

    #[test]
    fn test_undo_all_then_redo_all() {
        let mut mgr = UndoManager::in_memory();
        mgr.force_group_break();
        mgr.record(make_op(0, "a", ""));
        mgr.force_group_break();
        mgr.record(make_op(1, "b", ""));
        mgr.force_group_break();
        mgr.record(make_op(2, "c", ""));

        // Undo all
        mgr.undo(); // c
        mgr.undo(); // b
        mgr.undo(); // a
        assert!(!mgr.can_undo());

        // Redo all
        let ops = mgr.redo().expect("redo");
        assert_eq!(ops[0].inserted, "a");
        let ops = mgr.redo().expect("redo");
        assert_eq!(ops[0].inserted, "b");
        let ops = mgr.redo().expect("redo");
        assert_eq!(ops[0].inserted, "c");
        assert!(!mgr.can_redo());
    }

    #[test]
    fn test_clear() {
        let mut mgr = UndoManager::in_memory();
        mgr.record(make_op(0, "a", ""));
        mgr.clear().expect("clear");
        assert!(!mgr.can_undo());
        assert!(!mgr.can_redo());
    }

    #[test]
    fn test_doc_id() {
        let mgr = UndoManager::new("my-doc".to_string(), HistoryConfig::default(), None);
        assert_eq!(mgr.doc_id(), "my-doc");
    }

    // --- In-memory capacity limit ---

    #[test]
    fn test_in_memory_max_depth_enforced() {
        let config = HistoryConfig {
            hot_capacity: 5,
            max_history_depth: 10,
            group_timeout_ms: 500,
            data_dir: std::path::PathBuf::from("."),
        };
        let mut mgr = UndoManager::new("test".to_string(), config, None);

        for i in 0..20 {
            mgr.force_group_break();
            mgr.record(make_op(i, &format!("op{i}"), ""));
        }

        // Should be capped at max_history_depth
        assert!(mgr.hot_undo.len() <= 10);
    }

    // --- Persistence tests ---

    #[test]
    fn test_flush_writes_to_disk() {
        let dir = TempDir::new().expect("create temp dir");
        let (mut mgr, pl) = persistent_manager(dir.path());

        mgr.force_group_break();
        mgr.record(make_op(0, "hello", ""));
        mgr.flush().expect("flush");

        let groups = pl.read_groups("test-doc").expect("read");
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].operations[0].inserted, "hello");
    }

    #[test]
    fn test_flush_noop_when_not_dirty() {
        let dir = TempDir::new().expect("create temp dir");
        let (mut mgr, _pl) = persistent_manager(dir.path());

        // No edits = not dirty
        mgr.flush().expect("flush");
        // Should succeed without error
    }

    #[test]
    fn test_spill_to_disk_on_overflow() {
        let dir = TempDir::new().expect("create temp dir");
        let (mut mgr, pl) = persistent_manager(dir.path());

        // Record more than hot_capacity (5) groups
        for i in 0..8 {
            mgr.force_group_break();
            mgr.record(make_op(i, &format!("g{i}"), ""));
        }

        // Some should have spilled to disk
        assert!(mgr.cold_count > 0);
        let on_disk = pl.count_groups("test-doc").expect("count");
        assert!(on_disk > 0);

        // Hot cache should be under capacity
        assert!(mgr.hot_undo.len() <= 5);
    }

    #[test]
    fn test_load_from_cold_on_deep_undo() {
        let dir = TempDir::new().expect("create temp dir");
        let (mut mgr, _pl) = persistent_manager(dir.path());

        // Record enough to trigger spill
        for i in 0..8 {
            mgr.force_group_break();
            mgr.record(make_op(i, &format!("g{i}"), ""));
        }

        let total_groups = mgr.hot_undo.len() + mgr.cold_count;

        // Undo all — should load from cold when hot is empty
        let mut undo_count = 0;
        while mgr.can_undo() {
            mgr.undo();
            undo_count += 1;
        }

        assert_eq!(undo_count, total_groups);
        assert!(!mgr.can_undo());
    }

    #[test]
    fn test_load_or_new_restores_history() {
        let dir = TempDir::new().expect("create temp dir");

        // Create and populate a manager, then flush
        {
            let pl = PersistenceLayer::open(dir.path()).expect("open");
            let config = small_config();
            let mut mgr =
                UndoManager::new("restore-doc".to_string(), config, Some(Arc::clone(&pl)));

            mgr.force_group_break();
            mgr.record(make_op(0, "first", ""));
            mgr.force_group_break();
            mgr.record(make_op(5, "second", ""));
            mgr.flush().expect("flush");
        }

        // Load from disk
        {
            let pl = PersistenceLayer::open(dir.path()).expect("reopen");
            let config = small_config();
            let mut mgr = UndoManager::load_or_new("restore-doc".to_string(), config, Some(pl))
                .expect("load");

            assert!(mgr.can_undo());

            let ops = mgr.undo().expect("undo");
            assert_eq!(ops[0].inserted, "second");

            let ops = mgr.undo().expect("undo");
            assert_eq!(ops[0].inserted, "first");

            assert!(!mgr.can_undo());
        }
    }

    #[test]
    fn test_load_or_new_fresh_document() {
        let dir = TempDir::new().expect("create temp dir");
        let pl = PersistenceLayer::open(dir.path()).expect("open");
        let config = small_config();

        let mgr = UndoManager::load_or_new("new-doc".to_string(), config, Some(pl)).expect("load");

        assert!(!mgr.can_undo());
        assert!(!mgr.can_redo());
    }

    #[test]
    fn test_delete_history_clears_disk() {
        let dir = TempDir::new().expect("create temp dir");
        let (mut mgr, pl) = persistent_manager(dir.path());

        mgr.force_group_break();
        mgr.record(make_op(0, "data", ""));
        mgr.flush().expect("flush");

        assert_eq!(pl.count_groups("test-doc").expect("count"), 1);

        mgr.delete_history().expect("delete");

        assert_eq!(pl.count_groups("test-doc").expect("count"), 0);
        assert!(!mgr.can_undo());
    }

    #[test]
    fn test_max_history_depth_eviction() {
        let dir = TempDir::new().expect("create temp dir");
        let pl = PersistenceLayer::open(dir.path()).expect("open");
        let config = HistoryConfig {
            hot_capacity: 3,
            max_history_depth: 8,
            group_timeout_ms: 500,
            data_dir: dir.path().to_path_buf(),
        };
        let mut mgr = UndoManager::new("evict-doc".to_string(), config, Some(Arc::clone(&pl)));

        // Record 15 groups — should trigger spill and eviction
        for i in 0..15 {
            mgr.force_group_break();
            mgr.record(make_op(i, &format!("g{i}"), ""));
        }

        // Total (hot + cold) should not exceed max_history_depth
        let total = mgr.hot_undo.len() + mgr.cold_count;
        assert!(total <= 8, "total {total} exceeds max_history_depth 8");
    }

    #[test]
    fn test_undo_past_cold_with_redo_filtering() {
        let dir = TempDir::new().expect("create temp dir");
        let (mut mgr, _pl) = persistent_manager(dir.path());

        // Record enough to spill
        for i in 0..8 {
            mgr.force_group_break();
            mgr.record(make_op(i, &format!("g{i}"), ""));
        }

        // Flush so everything is on disk
        mgr.flush().expect("flush");

        // Undo the hot groups (moves them to redo)
        let hot_count = mgr.hot_undo.len();
        for _ in 0..hot_count {
            mgr.undo();
        }

        // Now undo should load from cold, filtering redo duplicates
        if mgr.can_undo() {
            let ops = mgr.undo().expect("cold undo");
            assert!(!ops.is_empty());
        }

        // All undone items should be redo-able
        while mgr.can_redo() {
            mgr.redo();
        }

        // Verify we can undo everything again
        let mut count = 0;
        while mgr.can_undo() {
            mgr.undo();
            count += 1;
        }
        assert!(count > 0);
    }

    #[test]
    fn test_multiple_documents_independent() {
        let dir = TempDir::new().expect("create temp dir");
        let pl = PersistenceLayer::open(dir.path()).expect("open");
        let config = small_config();

        let mut mgr_a =
            UndoManager::new("doc-a".to_string(), config.clone(), Some(Arc::clone(&pl)));
        let mut mgr_b = UndoManager::new("doc-b".to_string(), config, Some(Arc::clone(&pl)));

        mgr_a.force_group_break();
        mgr_a.record(make_op(0, "alpha", ""));
        mgr_b.force_group_break();
        mgr_b.record(make_op(0, "beta", ""));

        mgr_a.flush().expect("flush a");
        mgr_b.flush().expect("flush b");

        // Delete doc-a, doc-b should be unaffected
        mgr_a.delete_history().expect("delete a");

        assert!(!mgr_a.can_undo());
        assert!(mgr_b.can_undo());
    }
}
