// Integration tests for the history system.
//
// These tests exercise full workflows spanning the UndoManager and
// PersistenceLayer together, simulating realistic usage patterns.

use std::sync::Arc;

use rust_pad_mod_history::{
    CursorSnapshot, EditOperation, HistoryConfig, PersistenceLayer, UndoManager,
};

fn cursor(line: usize, col: usize) -> CursorSnapshot {
    CursorSnapshot { line, col }
}

fn make_op(pos: usize, inserted: &str, deleted: &str) -> EditOperation {
    EditOperation {
        position: pos,
        inserted: inserted.to_string(),
        deleted: deleted.to_string(),
        cursor_before: cursor(0, pos),
        cursor_after: cursor(0, pos + inserted.len()),
    }
}

fn test_config(dir: &std::path::Path) -> HistoryConfig {
    HistoryConfig {
        hot_capacity: 10,
        max_history_depth: 100,
        group_timeout_ms: 0, // immediate grouping break for deterministic tests
        data_dir: dir.to_path_buf(),
    }
}

fn new_mgr(doc_id: &str, pl: &Arc<PersistenceLayer>, config: &HistoryConfig) -> UndoManager {
    UndoManager::load_or_new(doc_id.to_string(), config.clone(), Some(Arc::clone(pl))).unwrap()
}

// ── Full Workflow ──────────────────────────────────────────────────────

#[test]
fn test_full_workflow_record_undo_flush_reload_undo() {
    let dir = tempfile::tempdir().unwrap();
    let config = HistoryConfig {
        hot_capacity: 50,
        max_history_depth: 1000,
        group_timeout_ms: 0,
        data_dir: dir.path().to_path_buf(),
    };
    let pl = PersistenceLayer::open(dir.path()).unwrap();

    // Phase 1: record 100 edits
    let mut mgr = new_mgr("test-full-workflow", &pl, &config);
    for i in 0..100 {
        mgr.record(make_op(i, &format!("char{i}"), ""));
        mgr.force_group_break();
    }
    assert!(mgr.can_undo());

    // Phase 2: undo 50
    for _ in 0..50 {
        assert!(mgr.undo().is_some());
    }

    // Phase 3: flush and drop
    mgr.flush().unwrap();
    drop(mgr);

    // Phase 4: reload from disk
    let mut mgr2 = new_mgr("test-full-workflow", &pl, &config);
    assert!(mgr2.can_undo());
    // Redo stack is NOT persisted
    assert!(!mgr2.can_redo());

    // Phase 5: undo the remaining 50
    for _ in 0..50 {
        assert!(mgr2.undo().is_some());
    }
    assert!(!mgr2.can_undo());
}

// ── Multi-Document Isolation ───────────────────────────────────────────

#[test]
fn test_multi_document_10_documents_same_database() {
    let dir = tempfile::tempdir().unwrap();
    let config = test_config(dir.path());
    let pl = PersistenceLayer::open(dir.path()).unwrap();

    // Create 10 managers writing to the same database
    let mut managers: Vec<UndoManager> = (0..10)
        .map(|i| new_mgr(&format!("doc-{i}"), &pl, &config))
        .collect();

    // Record different edits in each
    for (i, mgr) in managers.iter_mut().enumerate() {
        for j in 0..20 {
            mgr.record(make_op(j, &format!("d{i}e{j}"), ""));
            mgr.force_group_break();
        }
    }

    // Flush all
    for mgr in &mut managers {
        mgr.flush().unwrap();
    }

    // Verify each document has its own history
    for mgr in &mut managers {
        let mut undo_count = 0;
        while mgr.undo().is_some() {
            undo_count += 1;
        }
        assert_eq!(undo_count, 20);
    }

    // Verify persistence layer lists all 10 documents
    let docs = pl.list_documents().unwrap();
    assert_eq!(docs.len(), 10);
}

#[test]
fn test_multi_document_delete_one_preserves_others() {
    let dir = tempfile::tempdir().unwrap();
    let config = test_config(dir.path());
    let pl = PersistenceLayer::open(dir.path()).unwrap();

    let mut mgr_a = new_mgr("doc-a", &pl, &config);
    let mut mgr_b = new_mgr("doc-b", &pl, &config);

    for i in 0..5 {
        mgr_a.record(make_op(i, "a", ""));
        mgr_a.force_group_break();
        mgr_b.record(make_op(i, "b", ""));
        mgr_b.force_group_break();
    }
    mgr_a.flush().unwrap();
    mgr_b.flush().unwrap();

    // Delete doc-a
    mgr_a.delete_history().unwrap();
    drop(mgr_a);

    // doc-b should still be intact
    let count = pl.count_groups("doc-b").unwrap();
    assert_eq!(count, 5);

    // doc-a should be gone
    let count_a = pl.count_groups("doc-a").unwrap();
    assert_eq!(count_a, 0);
}

// ── Large Payload Handling ─────────────────────────────────────────────

#[test]
fn test_large_edit_100kb_payload() {
    let dir = tempfile::tempdir().unwrap();
    let config = test_config(dir.path());
    let pl = PersistenceLayer::open(dir.path()).unwrap();

    let large_text = "x".repeat(100_000);
    let mut mgr = new_mgr("large-doc", &pl, &config);

    mgr.record(make_op(0, &large_text, ""));
    mgr.force_group_break();
    mgr.flush().unwrap();

    // Reload and verify
    drop(mgr);
    let mut mgr2 = new_mgr("large-doc", &pl, &config);
    let ops = mgr2.undo().unwrap();
    assert_eq!(ops[0].inserted.len(), 100_000);
}

#[test]
fn test_large_edit_multiple_groups_with_big_deletes() {
    let dir = tempfile::tempdir().unwrap();
    let config = test_config(dir.path());
    let pl = PersistenceLayer::open(dir.path()).unwrap();

    let big_delete = "y".repeat(200_000);
    let mut mgr = new_mgr("big-delete", &pl, &config);

    for _ in 0..5 {
        mgr.record(make_op(0, "", &big_delete));
        mgr.force_group_break();
    }
    mgr.flush().unwrap();
    drop(mgr);

    let mut mgr2 = new_mgr("big-delete", &pl, &config);
    for _ in 0..5 {
        let ops = mgr2.undo().unwrap();
        assert_eq!(ops[0].deleted.len(), 200_000);
    }
    assert!(!mgr2.can_undo());
}

// ── Edge Cases ─────────────────────────────────────────────────────────

#[test]
fn test_undo_on_empty_history() {
    let dir = tempfile::tempdir().unwrap();
    let config = test_config(dir.path());
    let pl = PersistenceLayer::open(dir.path()).unwrap();

    let mut mgr = new_mgr("empty", &pl, &config);
    assert!(!mgr.can_undo());
    assert!(!mgr.can_redo());
    assert!(mgr.undo().is_none());
    assert!(mgr.redo().is_none());
}

#[test]
fn test_redo_cleared_on_new_edit_after_undo() {
    let dir = tempfile::tempdir().unwrap();
    let config = test_config(dir.path());
    let pl = PersistenceLayer::open(dir.path()).unwrap();

    let mut mgr = new_mgr("redo-clear", &pl, &config);

    mgr.record(make_op(0, "a", ""));
    mgr.force_group_break();
    mgr.record(make_op(1, "b", ""));
    mgr.force_group_break();

    mgr.undo();
    assert!(mgr.can_redo());

    // New edit should clear redo
    mgr.record(make_op(1, "c", ""));
    mgr.force_group_break();
    assert!(!mgr.can_redo());
}

#[test]
fn test_clear_then_reload_gives_empty_history() {
    let dir = tempfile::tempdir().unwrap();
    let config = test_config(dir.path());
    let pl = PersistenceLayer::open(dir.path()).unwrap();

    let mut mgr = new_mgr("clear-test", &pl, &config);

    for i in 0..20 {
        mgr.record(make_op(i, "x", ""));
        mgr.force_group_break();
    }
    mgr.flush().unwrap();

    // Clear should wipe both memory and disk
    mgr.clear().unwrap();
    assert!(!mgr.can_undo());
    drop(mgr);

    // Reload should also be empty
    let mgr2 = new_mgr("clear-test", &pl, &config);
    assert!(!mgr2.can_undo());
    assert!(!mgr2.can_redo());
}

#[test]
fn test_spill_and_cold_load_cycle() {
    // Test that hot -> cold -> hot transitions work seamlessly
    let dir = tempfile::tempdir().unwrap();
    let config = HistoryConfig {
        hot_capacity: 5,
        max_history_depth: 50,
        group_timeout_ms: 0,
        data_dir: dir.path().to_path_buf(),
    };
    let pl = PersistenceLayer::open(dir.path()).unwrap();

    let mut mgr = new_mgr("spill-cycle", &pl, &config);

    // Record 30 edits -> will trigger multiple spills (hot cap = 5)
    for i in 0..30 {
        mgr.record(make_op(i, &format!("e{i}"), ""));
        mgr.force_group_break();
    }

    // Undo all 30 -> requires loading from cold storage
    let mut undo_count = 0;
    while mgr.undo().is_some() {
        undo_count += 1;
    }
    assert_eq!(undo_count, 30);
}

#[test]
fn test_database_reopen_after_crash_simulation() {
    // Simulate: write data, drop without flush, reopen
    let dir = tempfile::tempdir().unwrap();
    let config = HistoryConfig {
        hot_capacity: 3,
        max_history_depth: 50,
        group_timeout_ms: 0,
        data_dir: dir.path().to_path_buf(),
    };
    let pl = PersistenceLayer::open(dir.path()).unwrap();

    let mut mgr = new_mgr("crash-sim", &pl, &config);

    // Record enough to trigger spill (hot_capacity = 3)
    for i in 0..10 {
        mgr.record(make_op(i, &format!("v{i}"), ""));
        mgr.force_group_break();
    }

    // Spill should have written some groups to disk automatically
    // Now drop WITHOUT flushing (simulates crash)
    drop(mgr);
    drop(pl);

    // Reopen
    let pl2 = PersistenceLayer::open(dir.path()).unwrap();
    let mgr2 = new_mgr("crash-sim", &pl2, &config);

    // Should recover whatever was spilled (not the hot-only ones)
    // With hot_capacity=3, after 10 records, at least some should be on disk
    assert!(mgr2.can_undo());
}

#[test]
fn test_interleaved_undo_redo_across_reload() {
    let dir = tempfile::tempdir().unwrap();
    let config = test_config(dir.path());
    let pl = PersistenceLayer::open(dir.path()).unwrap();

    let mut mgr = new_mgr("interleave", &pl, &config);

    // Record A, B, C
    for ch in ["a", "b", "c"] {
        mgr.record(make_op(0, ch, ""));
        mgr.force_group_break();
    }

    // Undo C -> redo stack has C
    let ops = mgr.undo().unwrap();
    assert_eq!(ops[0].inserted, "c");
    assert!(mgr.can_redo());

    // Flush and reload
    mgr.flush().unwrap();
    drop(mgr);

    let mut mgr2 = new_mgr("interleave", &pl, &config);

    // After reload, redo is gone (not persisted)
    assert!(!mgr2.can_redo());
    // But undo should still work for A, B
    assert!(mgr2.can_undo());
    let ops = mgr2.undo().unwrap();
    assert_eq!(ops[0].inserted, "b");
    let ops = mgr2.undo().unwrap();
    assert_eq!(ops[0].inserted, "a");
    assert!(!mgr2.can_undo());
}
