/// Core types for edit operations and groups.
use serde::{Deserialize, Serialize};

/// Cursor position snapshot for serialization.
///
/// Mirrors the `Position` type from `rust-pad-core` but is independently
/// serializable without depending on the core crate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct CursorSnapshot {
    /// 0-indexed line number.
    pub line: usize,
    /// 0-indexed column (char offset within line).
    pub col: usize,
}

/// A single atomic edit operation that can be undone/redone.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditOperation {
    /// Char index where the edit occurred.
    pub position: usize,
    /// Text that was inserted (empty for pure deletions).
    pub inserted: String,
    /// Text that was deleted (empty for pure insertions).
    pub deleted: String,
    /// Cursor state before the edit.
    pub cursor_before: CursorSnapshot,
    /// Cursor state after the edit.
    pub cursor_after: CursorSnapshot,
}

/// A group of operations that form a single undo step.
///
/// Consecutive edits within the grouping timeout are merged into
/// one group so they undo/redo as a single action.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditGroup {
    /// Operations in this group, in chronological order.
    pub operations: Vec<EditOperation>,
    /// Monotonic sequence number assigned by the `UndoManager`.
    pub seq: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_op() -> EditOperation {
        EditOperation {
            position: 42,
            inserted: "hello".to_string(),
            deleted: "world".to_string(),
            cursor_before: CursorSnapshot { line: 1, col: 5 },
            cursor_after: CursorSnapshot { line: 1, col: 10 },
        }
    }

    #[test]
    fn test_cursor_snapshot_default() {
        let snap = CursorSnapshot::default();
        assert_eq!(snap.line, 0);
        assert_eq!(snap.col, 0);
    }

    #[test]
    fn test_edit_operation_serde_roundtrip() {
        let op = sample_op();
        let bytes = bincode::serialize(&op).expect("serialize");
        let decoded: EditOperation = bincode::deserialize(&bytes).expect("deserialize");
        assert_eq!(decoded.position, 42);
        assert_eq!(decoded.inserted, "hello");
        assert_eq!(decoded.deleted, "world");
        assert_eq!(decoded.cursor_before, CursorSnapshot { line: 1, col: 5 });
        assert_eq!(decoded.cursor_after, CursorSnapshot { line: 1, col: 10 });
    }

    #[test]
    fn test_edit_group_serde_roundtrip() {
        let group = EditGroup {
            operations: vec![sample_op(), sample_op()],
            seq: 99,
        };
        let bytes = bincode::serialize(&group).expect("serialize");
        let decoded: EditGroup = bincode::deserialize(&bytes).expect("deserialize");
        assert_eq!(decoded.seq, 99);
        assert_eq!(decoded.operations.len(), 2);
        assert_eq!(decoded.operations[0].position, 42);
    }

    #[test]
    fn test_empty_strings_serde_roundtrip() {
        let op = EditOperation {
            position: 0,
            inserted: String::new(),
            deleted: String::new(),
            cursor_before: CursorSnapshot::default(),
            cursor_after: CursorSnapshot::default(),
        };
        let bytes = bincode::serialize(&op).expect("serialize");
        let decoded: EditOperation = bincode::deserialize(&bytes).expect("deserialize");
        assert!(decoded.inserted.is_empty());
        assert!(decoded.deleted.is_empty());
    }

    #[test]
    fn test_large_text_serde_roundtrip() {
        let large_text = "x".repeat(100_000);
        let op = EditOperation {
            position: 0,
            inserted: large_text.clone(),
            deleted: String::new(),
            cursor_before: CursorSnapshot::default(),
            cursor_after: CursorSnapshot::default(),
        };
        let bytes = bincode::serialize(&op).expect("serialize");
        let decoded: EditOperation = bincode::deserialize(&bytes).expect("deserialize");
        assert_eq!(decoded.inserted.len(), 100_000);
    }
}
