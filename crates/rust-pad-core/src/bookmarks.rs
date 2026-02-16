/// Bookmark management for line-level bookmarks.
use std::collections::BTreeSet;

/// Manages bookmarks (marked lines) in a document.
#[derive(Debug, Clone, Default)]
pub struct BookmarkManager {
    /// Set of bookmarked line indices (0-indexed).
    bookmarked_lines: BTreeSet<usize>,
}

impl BookmarkManager {
    /// Creates a new empty bookmark manager.
    pub fn new() -> Self {
        Self::default()
    }

    /// Toggles a bookmark on the given line.
    pub fn toggle(&mut self, line: usize) {
        if !self.bookmarked_lines.remove(&line) {
            self.bookmarked_lines.insert(line);
        }
    }

    /// Returns whether a line is bookmarked.
    pub fn is_bookmarked(&self, line: usize) -> bool {
        self.bookmarked_lines.contains(&line)
    }

    /// Returns the next bookmarked line after the given line, wrapping around.
    pub fn next(&self, current_line: usize) -> Option<usize> {
        if self.bookmarked_lines.is_empty() {
            return None;
        }
        // Find first bookmark after current line
        self.bookmarked_lines
            .range((current_line + 1)..)
            .next()
            .or_else(|| self.bookmarked_lines.iter().next())
            .copied()
    }

    /// Returns the previous bookmarked line before the given line, wrapping around.
    pub fn prev(&self, current_line: usize) -> Option<usize> {
        if self.bookmarked_lines.is_empty() {
            return None;
        }
        // Find last bookmark before current line
        self.bookmarked_lines
            .range(..current_line)
            .next_back()
            .or_else(|| self.bookmarked_lines.iter().next_back())
            .copied()
    }

    /// Removes all bookmarks.
    pub fn clear(&mut self) {
        self.bookmarked_lines.clear();
    }

    /// Returns the number of bookmarks.
    pub fn count(&self) -> usize {
        self.bookmarked_lines.len()
    }

    /// Returns all bookmarked lines as a sorted slice.
    pub fn lines(&self) -> Vec<usize> {
        self.bookmarked_lines.iter().copied().collect()
    }

    /// Adjusts bookmarks when lines are inserted or removed.
    pub fn adjust_for_edit(&mut self, line: usize, lines_added: isize) {
        let old: Vec<usize> = self.bookmarked_lines.iter().copied().collect();
        self.bookmarked_lines.clear();

        for bm_line in old {
            if bm_line < line {
                self.bookmarked_lines.insert(bm_line);
            } else if lines_added >= 0 {
                self.bookmarked_lines.insert(bm_line + lines_added as usize);
            } else {
                let removed = (-lines_added) as usize;
                if bm_line >= line + removed {
                    self.bookmarked_lines.insert(bm_line - removed);
                }
                // Bookmarks on removed lines are dropped
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_toggle() {
        let mut bm = BookmarkManager::new();
        bm.toggle(5);
        assert!(bm.is_bookmarked(5));
        bm.toggle(5);
        assert!(!bm.is_bookmarked(5));
    }

    #[test]
    fn test_next_prev() {
        let mut bm = BookmarkManager::new();
        bm.toggle(2);
        bm.toggle(5);
        bm.toggle(10);

        assert_eq!(bm.next(0), Some(2));
        assert_eq!(bm.next(5), Some(10));
        assert_eq!(bm.next(10), Some(2)); // Wrap around

        assert_eq!(bm.prev(10), Some(5));
        assert_eq!(bm.prev(2), Some(10)); // Wrap around
    }

    #[test]
    fn test_clear() {
        let mut bm = BookmarkManager::new();
        bm.toggle(1);
        bm.toggle(2);
        bm.clear();
        assert_eq!(bm.count(), 0);
    }

    #[test]
    fn test_adjust_for_insert() {
        let mut bm = BookmarkManager::new();
        bm.toggle(5);
        bm.toggle(10);
        bm.adjust_for_edit(3, 2); // Insert 2 lines at line 3
        assert!(bm.is_bookmarked(7));
        assert!(bm.is_bookmarked(12));
    }

    #[test]
    fn test_adjust_for_delete() {
        let mut bm = BookmarkManager::new();
        bm.toggle(5);
        bm.toggle(10);
        bm.adjust_for_edit(3, -2); // Remove 2 lines at line 3
        assert!(bm.is_bookmarked(3)); // 5 -> 3
        assert!(bm.is_bookmarked(8)); // 10 -> 8
    }

    // ── count() and lines() ──────────────────────────────────────────

    #[test]
    fn test_count_empty() {
        let bm = BookmarkManager::new();
        assert_eq!(bm.count(), 0);
    }

    #[test]
    fn test_count_after_add_and_remove() {
        let mut bm = BookmarkManager::new();
        bm.toggle(1);
        bm.toggle(5);
        bm.toggle(10);
        assert_eq!(bm.count(), 3);
        bm.toggle(5); // remove
        assert_eq!(bm.count(), 2);
    }

    #[test]
    fn test_lines_returns_sorted() {
        let mut bm = BookmarkManager::new();
        bm.toggle(10);
        bm.toggle(3);
        bm.toggle(7);
        assert_eq!(bm.lines(), vec![3, 7, 10]);
    }

    #[test]
    fn test_lines_empty() {
        let bm = BookmarkManager::new();
        assert!(bm.lines().is_empty());
    }

    // ── is_bookmarked edge cases ─────────────────────────────────────

    #[test]
    fn test_is_bookmarked_false_for_unset_line() {
        let mut bm = BookmarkManager::new();
        bm.toggle(5);
        assert!(!bm.is_bookmarked(0));
        assert!(!bm.is_bookmarked(4));
        assert!(!bm.is_bookmarked(6));
    }

    // ── next/prev with single bookmark ───────────────────────────────

    #[test]
    fn test_next_single_bookmark() {
        let mut bm = BookmarkManager::new();
        bm.toggle(5);
        // From before: goes to 5
        assert_eq!(bm.next(0), Some(5));
        // From 5 itself: wraps to 5 (only bookmark)
        assert_eq!(bm.next(5), Some(5));
        // From after: wraps to 5
        assert_eq!(bm.next(10), Some(5));
    }

    #[test]
    fn test_prev_single_bookmark() {
        let mut bm = BookmarkManager::new();
        bm.toggle(5);
        // From after: goes to 5
        assert_eq!(bm.prev(10), Some(5));
        // From 5 itself: wraps to 5 (only bookmark)
        assert_eq!(bm.prev(5), Some(5));
        // From before: wraps to 5
        assert_eq!(bm.prev(0), Some(5));
    }

    // ── next/prev with no bookmarks ──────────────────────────────────

    #[test]
    fn test_next_no_bookmarks() {
        let bm = BookmarkManager::new();
        assert_eq!(bm.next(0), None);
    }

    #[test]
    fn test_prev_no_bookmarks() {
        let bm = BookmarkManager::new();
        assert_eq!(bm.prev(0), None);
    }

    // ── adjust_for_edit edge cases ───────────────────────────────────

    #[test]
    fn test_adjust_for_edit_bookmark_on_removed_line() {
        let mut bm = BookmarkManager::new();
        bm.toggle(3);
        bm.toggle(5);
        bm.toggle(8);
        bm.adjust_for_edit(3, -3); // Remove 3 lines starting at line 3
                                   // Removed range: lines 3..6
                                   // bm 3: in removed range (3 <= 3 < 6) -> dropped
                                   // bm 5: in removed range (3 <= 5 < 6) -> dropped
                                   // bm 8: after removed range (8 >= 6) -> 8 - 3 = 5
        assert_eq!(bm.count(), 1);
        assert!(bm.is_bookmarked(5));
    }

    #[test]
    fn test_adjust_for_edit_zero_delta() {
        let mut bm = BookmarkManager::new();
        bm.toggle(3);
        bm.toggle(7);
        bm.adjust_for_edit(5, 0); // No change
        assert!(bm.is_bookmarked(3));
        assert!(bm.is_bookmarked(7));
    }

    #[test]
    fn test_adjust_for_edit_insert_at_bookmarked_line() {
        let mut bm = BookmarkManager::new();
        bm.toggle(5);
        bm.adjust_for_edit(5, 2); // Insert 2 lines at line 5
                                  // Bookmark at line 5 is >= edit line, so shifts to 7
        assert!(bm.is_bookmarked(7));
        assert!(!bm.is_bookmarked(5));
    }

    #[test]
    fn test_adjust_for_edit_bookmarks_before_edit_unchanged() {
        let mut bm = BookmarkManager::new();
        bm.toggle(2);
        bm.toggle(4);
        bm.adjust_for_edit(5, 3); // Insert 3 lines at line 5
                                  // Both bookmarks are before the edit line, unchanged
        assert!(bm.is_bookmarked(2));
        assert!(bm.is_bookmarked(4));
    }

    // ── toggle idempotency ───────────────────────────────────────────

    #[test]
    fn test_toggle_twice_removes() {
        let mut bm = BookmarkManager::new();
        bm.toggle(5);
        bm.toggle(5);
        assert!(!bm.is_bookmarked(5));
        assert_eq!(bm.count(), 0);
    }

    // ── clear after add ──────────────────────────────────────────────

    #[test]
    fn test_clear_resets_all_state() {
        let mut bm = BookmarkManager::new();
        bm.toggle(1);
        bm.toggle(5);
        bm.toggle(10);
        assert_eq!(bm.count(), 3);
        bm.clear();
        assert_eq!(bm.count(), 0);
        assert!(bm.lines().is_empty());
        assert_eq!(bm.next(0), None);
        assert_eq!(bm.prev(100), None);
    }
}
