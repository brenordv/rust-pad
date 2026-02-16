/// Cursor and selection model for text editing.
use crate::buffer::TextBuffer;
use anyhow::{Context, Result};

/// Represents a position in the text as line and column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Position {
    /// 0-indexed line number.
    pub line: usize,
    /// 0-indexed column (char offset within the line).
    pub col: usize,
}

impl Position {
    pub fn new(line: usize, col: usize) -> Self {
        Self { line, col }
    }
}

impl PartialOrd for Position {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Position {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.line.cmp(&other.line).then(self.col.cmp(&other.col))
    }
}

/// A selection range within the text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Selection {
    /// The anchor (start) of the selection.
    pub anchor: Position,
    /// The head (end / cursor) of the selection.
    pub head: Position,
}

impl Selection {
    /// Returns the start (min) position of the selection.
    pub fn start(&self) -> Position {
        std::cmp::min(self.anchor, self.head)
    }

    /// Returns the end (max) position of the selection.
    pub fn end(&self) -> Position {
        std::cmp::max(self.anchor, self.head)
    }

    /// Returns true if this selection is empty (anchor == head).
    pub fn is_empty(&self) -> bool {
        self.anchor == self.head
    }
}

/// The cursor state for a document, tracking position and optional selection.
#[derive(Debug, Clone)]
pub struct Cursor {
    /// Current cursor position.
    pub position: Position,
    /// Optional selection anchor. When set, selection is from anchor to position.
    pub selection_anchor: Option<Position>,
    /// Desired column when moving up/down (sticky column).
    pub desired_col: Option<usize>,
}

impl Default for Cursor {
    fn default() -> Self {
        Self::new()
    }
}

impl Cursor {
    /// Creates a new cursor at position (0, 0).
    pub fn new() -> Self {
        Self {
            position: Position::default(),
            selection_anchor: None,
            desired_col: None,
        }
    }

    /// Returns the current selection, if any.
    pub fn selection(&self) -> Option<Selection> {
        self.selection_anchor.map(|anchor| Selection {
            anchor,
            head: self.position,
        })
    }

    /// Returns the selection range as char offsets (start, end), ordered.
    /// If no selection, returns None.
    pub fn selection_char_range(&self, buffer: &TextBuffer) -> Result<Option<(usize, usize)>> {
        match self.selection_anchor {
            Some(anchor) => {
                let anchor_char = pos_to_char(buffer, anchor)?;
                let head_char = pos_to_char(buffer, self.position)?;
                let start = anchor_char.min(head_char);
                let end = anchor_char.max(head_char);
                Ok(Some((start, end)))
            }
            None => Ok(None),
        }
    }

    /// Converts the cursor position to a char index.
    pub fn to_char_index(&self, buffer: &TextBuffer) -> Result<usize> {
        pos_to_char(buffer, self.position)
    }

    /// Starts or extends selection from the current position.
    pub fn start_selection(&mut self) {
        if self.selection_anchor.is_none() {
            self.selection_anchor = Some(self.position);
        }
    }

    /// Clears the selection.
    pub fn clear_selection(&mut self) {
        self.selection_anchor = None;
    }

    /// Moves the cursor to an absolute position, clamped to buffer bounds.
    pub fn move_to(&mut self, pos: Position, buffer: &TextBuffer) {
        self.position = clamp_position(pos, buffer);
        self.desired_col = None;
    }

    /// Moves the cursor right by one character.
    pub fn move_right(&mut self, buffer: &TextBuffer) {
        let line_len = buffer.line_len_chars(self.position.line).unwrap_or(0);
        if self.position.col < line_len {
            self.position.col += 1;
        } else if self.position.line + 1 < buffer.len_lines() {
            self.position.line += 1;
            self.position.col = 0;
        }
        self.desired_col = None;
    }

    /// Moves the cursor left by one character.
    pub fn move_left(&mut self, buffer: &TextBuffer) {
        if self.position.col > 0 {
            self.position.col -= 1;
        } else if self.position.line > 0 {
            self.position.line -= 1;
            self.position.col = buffer.line_len_chars(self.position.line).unwrap_or(0);
        }
        self.desired_col = None;
    }

    /// Moves the cursor up by one line, preserving the desired column.
    pub fn move_up(&mut self, buffer: &TextBuffer) {
        if self.position.line == 0 {
            return;
        }
        let desired = self.desired_col.unwrap_or(self.position.col);
        self.position.line -= 1;
        let line_len = buffer.line_len_chars(self.position.line).unwrap_or(0);
        self.position.col = desired.min(line_len);
        self.desired_col = Some(desired);
    }

    /// Moves the cursor down by one line, preserving the desired column.
    pub fn move_down(&mut self, buffer: &TextBuffer) {
        if self.position.line + 1 >= buffer.len_lines() {
            return;
        }
        let desired = self.desired_col.unwrap_or(self.position.col);
        self.position.line += 1;
        let line_len = buffer.line_len_chars(self.position.line).unwrap_or(0);
        self.position.col = desired.min(line_len);
        self.desired_col = Some(desired);
    }

    /// Moves the cursor to the beginning of the current line.
    pub fn move_to_line_start(&mut self) {
        self.position.col = 0;
        self.desired_col = None;
    }

    /// Moves the cursor to the end of the current line.
    pub fn move_to_line_end(&mut self, buffer: &TextBuffer) {
        let line_len = buffer.line_len_chars(self.position.line).unwrap_or(0);
        self.position.col = line_len;
        self.desired_col = None;
    }

    /// Moves the cursor to the beginning of the next word.
    pub fn move_word_right(&mut self, buffer: &TextBuffer) {
        let total_chars = buffer.len_chars();
        let mut char_idx = pos_to_char(buffer, self.position).unwrap_or(0);

        if char_idx >= total_chars {
            return;
        }

        // Skip current word characters
        while char_idx < total_chars {
            let ch = buffer.char_at(char_idx).unwrap_or(' ');
            if !ch.is_alphanumeric() && ch != '_' {
                break;
            }
            char_idx += 1;
        }
        // Skip whitespace/punctuation
        while char_idx < total_chars {
            let ch = buffer.char_at(char_idx).unwrap_or(' ');
            if ch.is_alphanumeric() || ch == '_' {
                break;
            }
            char_idx += 1;
        }

        self.position = char_to_pos(buffer, char_idx);
        self.desired_col = None;
    }

    /// Moves the cursor to the beginning of the previous word.
    pub fn move_word_left(&mut self, buffer: &TextBuffer) {
        let mut char_idx = pos_to_char(buffer, self.position).unwrap_or(0);

        if char_idx == 0 {
            return;
        }

        char_idx -= 1;

        // Skip whitespace/punctuation backwards
        while char_idx > 0 {
            let ch = buffer.char_at(char_idx).unwrap_or(' ');
            if ch.is_alphanumeric() || ch == '_' {
                break;
            }
            char_idx -= 1;
        }
        // Skip word characters backwards
        while char_idx > 0 {
            let prev_ch = buffer.char_at(char_idx - 1).unwrap_or(' ');
            if !prev_ch.is_alphanumeric() && prev_ch != '_' {
                break;
            }
            char_idx -= 1;
        }

        self.position = char_to_pos(buffer, char_idx);
        self.desired_col = None;
    }

    /// Moves the cursor to the beginning of the document.
    pub fn move_to_start(&mut self) {
        self.position = Position::default();
        self.desired_col = None;
    }

    /// Moves the cursor to the end of the document.
    pub fn move_to_end(&mut self, buffer: &TextBuffer) {
        if buffer.len_lines() == 0 {
            self.position = Position::default();
        } else {
            let last_line = buffer.len_lines() - 1;
            let line_len = buffer.line_len_chars(last_line).unwrap_or(0);
            self.position = Position::new(last_line, line_len);
        }
        self.desired_col = None;
    }

    /// Selects all text in the buffer.
    pub fn select_all(&mut self, buffer: &TextBuffer) {
        self.selection_anchor = Some(Position::default());
        self.move_to_end(buffer);
    }

    /// Selects the current word.
    pub fn select_word(&mut self, buffer: &TextBuffer) {
        let char_idx = pos_to_char(buffer, self.position).unwrap_or(0);
        let total = buffer.len_chars();

        if total == 0 {
            return;
        }

        let idx = char_idx.min(total - 1);
        let ch = buffer.char_at(idx).unwrap_or(' ');

        if !ch.is_alphanumeric() && ch != '_' {
            return;
        }

        // Find word start
        let mut start = idx;
        while start > 0 {
            let prev = buffer.char_at(start - 1).unwrap_or(' ');
            if !prev.is_alphanumeric() && prev != '_' {
                break;
            }
            start -= 1;
        }

        // Find word end
        let mut end = idx + 1;
        while end < total {
            let next = buffer.char_at(end).unwrap_or(' ');
            if !next.is_alphanumeric() && next != '_' {
                break;
            }
            end += 1;
        }

        self.selection_anchor = Some(char_to_pos(buffer, start));
        self.position = char_to_pos(buffer, end);
        self.desired_col = None;
    }

    /// Selects the current line.
    pub fn select_line(&mut self, buffer: &TextBuffer) {
        let line = self.position.line;
        self.selection_anchor = Some(Position::new(line, 0));

        if line + 1 < buffer.len_lines() {
            self.position = Position::new(line + 1, 0);
        } else {
            let line_len = buffer.line_len_chars(line).unwrap_or(0);
            self.position = Position::new(line, line_len);
        }
        self.desired_col = None;
    }

    /// Moves the cursor up by a page (N lines).
    pub fn move_page_up(&mut self, page_lines: usize, buffer: &TextBuffer) {
        let desired = self.desired_col.unwrap_or(self.position.col);
        self.position.line = self.position.line.saturating_sub(page_lines);
        let line_len = buffer.line_len_chars(self.position.line).unwrap_or(0);
        self.position.col = desired.min(line_len);
        self.desired_col = Some(desired);
    }

    /// Moves the cursor down by a page (N lines).
    pub fn move_page_down(&mut self, page_lines: usize, buffer: &TextBuffer) {
        let desired = self.desired_col.unwrap_or(self.position.col);
        let max_line = buffer.len_lines().saturating_sub(1);
        self.position.line = (self.position.line + page_lines).min(max_line);
        let line_len = buffer.line_len_chars(self.position.line).unwrap_or(0);
        self.position.col = desired.min(line_len);
        self.desired_col = Some(desired);
    }
}

/// Converts a `Position` to a char index in the buffer.
pub fn pos_to_char(buffer: &TextBuffer, pos: Position) -> Result<usize> {
    let clamped = clamp_position(pos, buffer);
    let line_start = buffer
        .line_to_char(clamped.line)
        .context("converting position to char index")?;
    Ok(line_start + clamped.col)
}

/// Converts a char index to a `Position`.
pub fn char_to_pos(buffer: &TextBuffer, char_idx: usize) -> Position {
    let clamped = char_idx.min(buffer.len_chars());
    let line = buffer.char_to_line(clamped).unwrap_or(0);
    let line_start = buffer.line_to_char(line).unwrap_or(0);
    Position::new(line, clamped - line_start)
}

/// Clamps a position to valid buffer bounds.
fn clamp_position(pos: Position, buffer: &TextBuffer) -> Position {
    if buffer.len_lines() == 0 {
        return Position::default();
    }
    let line = pos.line.min(buffer.len_lines() - 1);
    let line_len = buffer.line_len_chars(line).unwrap_or(0);
    let col = pos.col.min(line_len);
    Position::new(line, col)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_buffer() -> TextBuffer {
        TextBuffer::from("hello world\nfoo bar\nbaz")
    }

    #[test]
    fn test_cursor_move_right() {
        let buf = test_buffer();
        let mut cursor = Cursor::new();
        cursor.move_right(&buf);
        assert_eq!(cursor.position, Position::new(0, 1));
    }

    #[test]
    fn test_cursor_move_right_wraps() {
        let buf = test_buffer();
        let mut cursor = Cursor::new();
        cursor.position = Position::new(0, 11);
        cursor.move_right(&buf);
        assert_eq!(cursor.position, Position::new(1, 0));
    }

    #[test]
    fn test_cursor_move_left() {
        let buf = test_buffer();
        let mut cursor = Cursor::new();
        cursor.position = Position::new(0, 5);
        cursor.move_left(&buf);
        assert_eq!(cursor.position, Position::new(0, 4));
    }

    #[test]
    fn test_cursor_move_left_wraps() {
        let buf = test_buffer();
        let mut cursor = Cursor::new();
        cursor.position = Position::new(1, 0);
        cursor.move_left(&buf);
        assert_eq!(cursor.position, Position::new(0, 11));
    }

    #[test]
    fn test_cursor_move_up_down() {
        let buf = test_buffer();
        let mut cursor = Cursor::new();
        cursor.position = Position::new(0, 5);
        cursor.move_down(&buf);
        assert_eq!(cursor.position, Position::new(1, 5));
        cursor.move_down(&buf);
        assert_eq!(cursor.position, Position::new(2, 3)); // "baz" only 3 chars
        cursor.move_up(&buf);
        assert_eq!(cursor.position, Position::new(1, 5)); // sticky col restored
    }

    #[test]
    fn test_cursor_move_to_line_start_end() {
        let buf = test_buffer();
        let mut cursor = Cursor::new();
        cursor.position = Position::new(0, 5);
        cursor.move_to_line_end(&buf);
        assert_eq!(cursor.position, Position::new(0, 11));
        cursor.move_to_line_start();
        assert_eq!(cursor.position, Position::new(0, 0));
    }

    #[test]
    fn test_selection() {
        let buf = test_buffer();
        let mut cursor = Cursor::new();
        cursor.start_selection();
        cursor.move_right(&buf);
        cursor.move_right(&buf);
        let sel = cursor.selection().unwrap();
        assert_eq!(sel.start(), Position::new(0, 0));
        assert_eq!(sel.end(), Position::new(0, 2));
    }

    #[test]
    fn test_select_word() {
        let buf = test_buffer();
        let mut cursor = Cursor::new();
        cursor.position = Position::new(0, 2);
        cursor.select_word(&buf);
        let sel = cursor.selection().unwrap();
        assert_eq!(sel.start(), Position::new(0, 0));
        assert_eq!(sel.end(), Position::new(0, 5));
    }

    #[test]
    fn test_select_line() {
        let buf = test_buffer();
        let mut cursor = Cursor::new();
        cursor.position = Position::new(0, 3);
        cursor.select_line(&buf);
        let sel = cursor.selection().unwrap();
        assert_eq!(sel.start(), Position::new(0, 0));
        assert_eq!(sel.end(), Position::new(1, 0));
    }

    #[test]
    fn test_word_movement() {
        let buf = TextBuffer::from("hello world foo");
        let mut cursor = Cursor::new();
        cursor.move_word_right(&buf);
        assert_eq!(cursor.position, Position::new(0, 6));
        cursor.move_word_right(&buf);
        assert_eq!(cursor.position, Position::new(0, 12));
        cursor.move_word_left(&buf);
        assert_eq!(cursor.position, Position::new(0, 6));
    }

    #[test]
    fn test_page_up_down() {
        let buf = TextBuffer::from("a\nb\nc\nd\ne\nf\ng\nh\ni\nj");
        let mut cursor = Cursor::new();
        cursor.move_page_down(5, &buf);
        assert_eq!(cursor.position.line, 5);
        cursor.move_page_up(3, &buf);
        assert_eq!(cursor.position.line, 2);
    }

    #[test]
    fn test_select_all() {
        let buf = test_buffer();
        let mut cursor = Cursor::new();
        cursor.select_all(&buf);
        let range = cursor.selection_char_range(&buf).unwrap().unwrap();
        assert_eq!(range, (0, buf.len_chars()));
    }

    // ── Boundary movement no-ops ─────────────────────────────────────

    #[test]
    fn test_move_right_at_end_of_buffer() {
        let buf = TextBuffer::from("abc");
        let mut cursor = Cursor::new();
        cursor.position = Position::new(0, 3);
        cursor.move_right(&buf);
        // Single line, at end — should not move
        assert_eq!(cursor.position, Position::new(0, 3));
    }

    #[test]
    fn test_move_left_at_start_of_buffer() {
        let buf = test_buffer();
        let mut cursor = Cursor::new();
        cursor.move_left(&buf);
        assert_eq!(cursor.position, Position::new(0, 0));
    }

    #[test]
    fn test_move_up_at_first_line() {
        let buf = test_buffer();
        let mut cursor = Cursor::new();
        cursor.position = Position::new(0, 5);
        cursor.move_up(&buf);
        // Already on line 0, should not move
        assert_eq!(cursor.position, Position::new(0, 5));
    }

    #[test]
    fn test_move_down_at_last_line() {
        let buf = test_buffer();
        let mut cursor = Cursor::new();
        cursor.position = Position::new(2, 1);
        cursor.move_down(&buf);
        // Already on last line, should not move
        assert_eq!(cursor.position, Position::new(2, 1));
    }

    // ── move_to clamping ─────────────────────────────────────────────

    #[test]
    fn test_move_to_clamps_line() {
        let buf = TextBuffer::from("abc\ndef");
        let mut cursor = Cursor::new();
        cursor.move_to(Position::new(100, 0), &buf);
        assert_eq!(cursor.position.line, 1); // clamped to last line
    }

    #[test]
    fn test_move_to_clamps_col() {
        let buf = TextBuffer::from("abc\ndef");
        let mut cursor = Cursor::new();
        cursor.move_to(Position::new(0, 100), &buf);
        assert_eq!(cursor.position.col, 3); // clamped to line length
    }

    #[test]
    fn test_move_to_empty_buffer() {
        let buf = TextBuffer::new();
        let mut cursor = Cursor::new();
        cursor.move_to(Position::new(10, 10), &buf);
        assert_eq!(cursor.position, Position::default());
    }

    // ── select_word edge cases ───────────────────────────────────────

    #[test]
    fn test_select_word_on_space() {
        let buf = TextBuffer::from("hello world");
        let mut cursor = Cursor::new();
        cursor.position = Position::new(0, 5); // on the space
        cursor.select_word(&buf);
        // No selection when cursor is on non-word character
        assert!(cursor.selection_anchor.is_none());
    }

    #[test]
    fn test_select_word_with_underscore() {
        let buf = TextBuffer::from("hello_world foo");
        let mut cursor = Cursor::new();
        cursor.position = Position::new(0, 3);
        cursor.select_word(&buf);
        let sel = cursor.selection().unwrap();
        assert_eq!(sel.start(), Position::new(0, 0));
        assert_eq!(sel.end(), Position::new(0, 11)); // "hello_world"
    }

    #[test]
    fn test_select_word_empty_buffer() {
        let buf = TextBuffer::new();
        let mut cursor = Cursor::new();
        cursor.select_word(&buf);
        assert!(cursor.selection_anchor.is_none());
    }

    // ── select_line edge cases ───────────────────────────────────────

    #[test]
    fn test_select_line_last_line() {
        let buf = TextBuffer::from("abc\ndef");
        let mut cursor = Cursor::new();
        cursor.position = Position::new(1, 1);
        cursor.select_line(&buf);
        let sel = cursor.selection().unwrap();
        // Last line: anchor at (1,0), head at (1, line_len)
        assert_eq!(sel.start(), Position::new(1, 0));
        assert_eq!(sel.end(), Position::new(1, 3));
    }

    #[test]
    fn test_select_line_single_line() {
        let buf = TextBuffer::from("hello");
        let mut cursor = Cursor::new();
        cursor.position = Position::new(0, 2);
        cursor.select_line(&buf);
        let sel = cursor.selection().unwrap();
        assert_eq!(sel.start(), Position::new(0, 0));
        assert_eq!(sel.end(), Position::new(0, 5));
    }

    // ── Selection helpers ────────────────────────────────────────────

    #[test]
    fn test_selection_char_range_no_selection() {
        let buf = test_buffer();
        let cursor = Cursor::new();
        assert!(cursor.selection_char_range(&buf).unwrap().is_none());
    }

    #[test]
    fn test_selection_char_range_reversed() {
        // Selection with head before anchor
        let buf = TextBuffer::from("hello world");
        let mut cursor = Cursor::new();
        cursor.selection_anchor = Some(Position::new(0, 5));
        cursor.position = Position::new(0, 0);
        let (start, end) = cursor.selection_char_range(&buf).unwrap().unwrap();
        assert_eq!(start, 0);
        assert_eq!(end, 5);
    }

    #[test]
    fn test_clear_selection() {
        let mut cursor = Cursor::new();
        cursor.start_selection();
        assert!(cursor.selection_anchor.is_some());
        cursor.clear_selection();
        assert!(cursor.selection_anchor.is_none());
    }

    #[test]
    fn test_start_selection_idempotent() {
        let mut cursor = Cursor::new();
        cursor.position = Position::new(0, 5);
        cursor.start_selection();
        let anchor = cursor.selection_anchor.unwrap();
        cursor.position = Position::new(0, 10);
        cursor.start_selection(); // should not change anchor
        assert_eq!(cursor.selection_anchor.unwrap(), anchor);
    }

    // ── Selection struct ─────────────────────────────────────────────

    #[test]
    fn test_selection_is_empty() {
        let sel = Selection {
            anchor: Position::new(0, 0),
            head: Position::new(0, 0),
        };
        assert!(sel.is_empty());

        let sel2 = Selection {
            anchor: Position::new(0, 0),
            head: Position::new(0, 5),
        };
        assert!(!sel2.is_empty());
    }

    #[test]
    fn test_selection_start_end_order() {
        let sel = Selection {
            anchor: Position::new(1, 0),
            head: Position::new(0, 5),
        };
        assert_eq!(sel.start(), Position::new(0, 5));
        assert_eq!(sel.end(), Position::new(1, 0));
    }

    // ── Position ordering ────────────────────────────────────────────

    #[test]
    fn test_position_ordering() {
        assert!(Position::new(0, 0) < Position::new(0, 1));
        assert!(Position::new(0, 5) < Position::new(1, 0));
        assert!(Position::new(1, 0) < Position::new(1, 1));
        assert_eq!(Position::new(2, 3), Position::new(2, 3));
    }

    // ── pos_to_char and char_to_pos ──────────────────────────────────

    #[test]
    fn test_pos_to_char_and_back() {
        let buf = TextBuffer::from("abc\ndef\nghi");
        // Position (1, 2) = 'd','e','f' -> char 6 (abc\n=4, de=2 more)
        let char_idx = pos_to_char(&buf, Position::new(1, 2)).unwrap();
        assert_eq!(char_idx, 6);
        let pos = char_to_pos(&buf, char_idx);
        assert_eq!(pos, Position::new(1, 2));
    }

    #[test]
    fn test_char_to_pos_at_end() {
        let buf = TextBuffer::from("abc");
        let pos = char_to_pos(&buf, 3);
        assert_eq!(pos, Position::new(0, 3));
    }

    #[test]
    fn test_char_to_pos_clamped() {
        let buf = TextBuffer::from("abc");
        let pos = char_to_pos(&buf, 100);
        assert_eq!(pos, Position::new(0, 3)); // clamped to end
    }

    // ── Word movement edge cases ─────────────────────────────────────

    #[test]
    fn test_move_word_right_at_end() {
        let buf = TextBuffer::from("abc");
        let mut cursor = Cursor::new();
        cursor.position = Position::new(0, 3);
        cursor.move_word_right(&buf);
        assert_eq!(cursor.position, Position::new(0, 3)); // no-op
    }

    #[test]
    fn test_move_word_left_at_start() {
        let buf = TextBuffer::from("abc");
        let mut cursor = Cursor::new();
        cursor.move_word_left(&buf);
        assert_eq!(cursor.position, Position::new(0, 0)); // no-op
    }

    #[test]
    fn test_move_word_across_lines() {
        let buf = TextBuffer::from("hello\nworld");
        let mut cursor = Cursor::new();
        cursor.move_word_right(&buf);
        // Should skip "hello" then skip "\n" to land at "world"
        assert_eq!(cursor.position, Position::new(1, 0));
    }

    // ── move_to_start / move_to_end ──────────────────────────────────

    #[test]
    fn test_move_to_start() {
        let _buf = test_buffer();
        let mut cursor = Cursor::new();
        cursor.position = Position::new(2, 2);
        cursor.move_to_start();
        assert_eq!(cursor.position, Position::new(0, 0));
    }

    #[test]
    fn test_move_to_end() {
        let buf = test_buffer(); // "hello world\nfoo bar\nbaz"
        let mut cursor = Cursor::new();
        cursor.move_to_end(&buf);
        assert_eq!(cursor.position, Position::new(2, 3)); // end of "baz"
    }

    #[test]
    fn test_move_to_end_empty_buffer() {
        let buf = TextBuffer::new();
        let mut cursor = Cursor::new();
        cursor.move_to_end(&buf);
        // Empty buffer with ropey has 1 line of length 0
        assert_eq!(cursor.position.line, 0);
        assert_eq!(cursor.position.col, 0);
    }

    // ── Page movement edge cases ─────────────────────────────────────

    #[test]
    fn test_page_up_past_beginning() {
        let buf = TextBuffer::from("a\nb\nc");
        let mut cursor = Cursor::new();
        cursor.position = Position::new(1, 0);
        cursor.move_page_up(10, &buf);
        assert_eq!(cursor.position.line, 0); // clamped to 0
    }

    #[test]
    fn test_page_down_past_end() {
        let buf = TextBuffer::from("a\nb\nc");
        let mut cursor = Cursor::new();
        cursor.move_page_down(100, &buf);
        assert_eq!(cursor.position.line, 2); // clamped to last line
    }

    // ── Sticky column ────────────────────────────────────────────────

    #[test]
    fn test_sticky_column_preserved_across_short_line() {
        let buf = TextBuffer::from("long line here\na\nlong line here");
        let mut cursor = Cursor::new();
        cursor.position = Position::new(0, 10);
        cursor.move_down(&buf); // line "a" has 1 char, col clamped to 1
        assert_eq!(cursor.position, Position::new(1, 1));
        cursor.move_down(&buf); // back to long line, col restored to 10
        assert_eq!(cursor.position, Position::new(2, 10));
    }
}
