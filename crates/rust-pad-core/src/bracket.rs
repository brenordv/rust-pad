//! Bracket matching: given a cursor position, find the matching bracket.
//!
//! Supports `()`, `[]`, and `{}`. Walks forward for opening brackets and
//! backward for closing brackets, tracking nesting depth. A configurable
//! search limit prevents freezing on very large files with unbalanced
//! brackets.

use crate::buffer::TextBuffer;

/// Maximum number of characters to scan when searching for a matching bracket.
const DEFAULT_SEARCH_LIMIT: usize = 10_000;

/// A matched pair of brackets with their character indices.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BracketPair {
    /// Char index of the opening bracket (`(`, `[`, or `{`).
    pub open: usize,
    /// Char index of the closing bracket (`)`, `]`, or `}`).
    pub close: usize,
}

/// Returns the matching closing bracket for an opening bracket.
fn closing_for(ch: char) -> Option<char> {
    match ch {
        '(' => Some(')'),
        '[' => Some(']'),
        '{' => Some('}'),
        _ => None,
    }
}

/// Returns the matching opening bracket for a closing bracket.
fn opening_for(ch: char) -> Option<char> {
    match ch {
        ')' => Some('('),
        ']' => Some('['),
        '}' => Some('{'),
        _ => None,
    }
}

/// Returns `true` if the character is any bracket.
fn is_bracket(ch: char) -> bool {
    matches!(ch, '(' | ')' | '[' | ']' | '{' | '}')
}

/// Finds the bracket matching the one at or adjacent to `cursor_char_idx`.
///
/// Checks the character at the cursor position first, then the character
/// before it. For an opening bracket, walks forward; for a closing bracket,
/// walks backward. Returns `None` if no bracket is adjacent, no match is
/// found within the search limit, or the buffer is empty.
pub fn find_matching_bracket(buffer: &TextBuffer, cursor_char_idx: usize) -> Option<BracketPair> {
    find_matching_bracket_with_limit(buffer, cursor_char_idx, DEFAULT_SEARCH_LIMIT)
}

/// Like [`find_matching_bracket`] but with a configurable search limit.
pub fn find_matching_bracket_with_limit(
    buffer: &TextBuffer,
    cursor_char_idx: usize,
    search_limit: usize,
) -> Option<BracketPair> {
    let total = buffer.len_chars();
    if total == 0 {
        return None;
    }

    // Check character at cursor position first, then character before cursor.
    if cursor_char_idx < total {
        if let Ok(ch) = buffer.char_at(cursor_char_idx) {
            if is_bracket(ch) {
                if let Some(pair) = try_match(buffer, cursor_char_idx, ch, search_limit) {
                    return Some(pair);
                }
            }
        }
    }

    if cursor_char_idx > 0 {
        let before = cursor_char_idx - 1;
        if let Ok(ch) = buffer.char_at(before) {
            if is_bracket(ch) {
                if let Some(pair) = try_match(buffer, before, ch, search_limit) {
                    return Some(pair);
                }
            }
        }
    }

    None
}

/// Attempts to find the matching bracket for `ch` at `pos`.
fn try_match(
    buffer: &TextBuffer,
    pos: usize,
    ch: char,
    search_limit: usize,
) -> Option<BracketPair> {
    if let Some(close_ch) = closing_for(ch) {
        // Opening bracket → walk forward
        match_forward(buffer, pos, ch, close_ch, search_limit)
    } else if let Some(open_ch) = opening_for(ch) {
        // Closing bracket → walk backward
        match_backward(buffer, pos, ch, open_ch, search_limit)
    } else {
        None
    }
}

/// Walks forward from `pos` to find the closing bracket.
fn match_forward(
    buffer: &TextBuffer,
    pos: usize,
    open_ch: char,
    close_ch: char,
    search_limit: usize,
) -> Option<BracketPair> {
    let total = buffer.len_chars();
    let mut depth: usize = 0;

    for offset in 0..=search_limit {
        let idx = pos + offset;
        if idx >= total {
            break;
        }
        let ch = buffer.char_at(idx).ok()?;
        if ch == open_ch {
            depth += 1;
        } else if ch == close_ch {
            depth -= 1;
            if depth == 0 {
                return Some(BracketPair {
                    open: pos,
                    close: idx,
                });
            }
        }
    }
    None
}

/// Walks backward from `pos` to find the opening bracket.
fn match_backward(
    buffer: &TextBuffer,
    pos: usize,
    close_ch: char,
    open_ch: char,
    search_limit: usize,
) -> Option<BracketPair> {
    let mut depth: usize = 0;
    let start = pos.saturating_sub(search_limit);

    for idx in (start..=pos).rev() {
        let ch = buffer.char_at(idx).ok()?;
        if ch == close_ch {
            depth += 1;
        } else if ch == open_ch {
            depth -= 1;
            if depth == 0 {
                return Some(BracketPair {
                    open: idx,
                    close: pos,
                });
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn buf(text: &str) -> TextBuffer {
        TextBuffer::from(text)
    }

    // ── Basic matching ──────────────────────────────────────────────

    #[test]
    fn match_open_paren_at_cursor() {
        let b = buf("(hello)");
        let pair = find_matching_bracket(&b, 0).unwrap();
        assert_eq!(pair, BracketPair { open: 0, close: 6 });
    }

    #[test]
    fn match_close_paren_at_cursor() {
        let b = buf("(hello)");
        let pair = find_matching_bracket(&b, 6).unwrap();
        assert_eq!(pair, BracketPair { open: 0, close: 6 });
    }

    #[test]
    fn match_close_paren_before_cursor() {
        // Cursor right after `)`, so cursor_char_idx = 7 (past the end of `)`).
        // Should check char before cursor (index 6 = `)`).
        let b = buf("(hello)");
        let pair = find_matching_bracket(&b, 7).unwrap();
        assert_eq!(pair, BracketPair { open: 0, close: 6 });
    }

    #[test]
    fn match_square_brackets() {
        let b = buf("[a, b]");
        let pair = find_matching_bracket(&b, 0).unwrap();
        assert_eq!(pair, BracketPair { open: 0, close: 5 });
    }

    #[test]
    fn match_curly_braces() {
        let b = buf("{x}");
        let pair = find_matching_bracket(&b, 0).unwrap();
        assert_eq!(pair, BracketPair { open: 0, close: 2 });
    }

    // ── Nested brackets ─────────────────────────────────────────────

    #[test]
    fn nested_parens() {
        let b = buf("((()))");
        // Outer open
        let pair = find_matching_bracket(&b, 0).unwrap();
        assert_eq!(pair, BracketPair { open: 0, close: 5 });
        // Middle open
        let pair = find_matching_bracket(&b, 1).unwrap();
        assert_eq!(pair, BracketPair { open: 1, close: 4 });
        // Inner open
        let pair = find_matching_bracket(&b, 2).unwrap();
        assert_eq!(pair, BracketPair { open: 2, close: 3 });
    }

    #[test]
    fn nested_parens_reverse() {
        let b = buf("((()))");
        // Inner close
        let pair = find_matching_bracket(&b, 3).unwrap();
        assert_eq!(pair, BracketPair { open: 2, close: 3 });
        // Middle close
        let pair = find_matching_bracket(&b, 4).unwrap();
        assert_eq!(pair, BracketPair { open: 1, close: 4 });
        // Outer close
        let pair = find_matching_bracket(&b, 5).unwrap();
        assert_eq!(pair, BracketPair { open: 0, close: 5 });
    }

    // ── Mixed bracket types ─────────────────────────────────────────

    #[test]
    fn mixed_types_each_matches_own_kind() {
        let b = buf("({[]})");
        // `(` at 0 matches `)` at 5
        let pair = find_matching_bracket(&b, 0).unwrap();
        assert_eq!(pair, BracketPair { open: 0, close: 5 });
        // `{` at 1 matches `}` at 4
        let pair = find_matching_bracket(&b, 1).unwrap();
        assert_eq!(pair, BracketPair { open: 1, close: 4 });
        // `[` at 2 matches `]` at 3
        let pair = find_matching_bracket(&b, 2).unwrap();
        assert_eq!(pair, BracketPair { open: 2, close: 3 });
    }

    // ── Unmatched / no bracket ──────────────────────────────────────

    #[test]
    fn unmatched_open_returns_none() {
        let b = buf("(hello");
        assert!(find_matching_bracket(&b, 0).is_none());
    }

    #[test]
    fn unmatched_close_returns_none() {
        let b = buf("hello)");
        assert!(find_matching_bracket(&b, 5).is_none());
    }

    #[test]
    fn cursor_not_on_bracket_returns_none() {
        let b = buf("hello");
        assert!(find_matching_bracket(&b, 2).is_none());
    }

    #[test]
    fn empty_buffer_returns_none() {
        let b = buf("");
        assert!(find_matching_bracket(&b, 0).is_none());
    }

    // ── Search limit ────────────────────────────────────────────────

    #[test]
    fn search_limit_exceeded_returns_none() {
        // Place matching bracket beyond the limit
        let text = format!("({})", "x".repeat(20));
        let b = buf(&text);
        // With limit of 5, the closing `)` at index 21 is out of reach
        assert!(find_matching_bracket_with_limit(&b, 0, 5).is_none());
    }

    #[test]
    fn search_limit_just_enough() {
        let text = format!("({})", "x".repeat(3));
        let b = buf(&text); // "(xxx)" — `)` is at index 4
        let pair = find_matching_bracket_with_limit(&b, 0, 4).unwrap();
        assert_eq!(pair, BracketPair { open: 0, close: 4 });
    }

    // ── Adjacent bracket preference ─────────────────────────────────

    #[test]
    fn prefers_char_at_cursor_over_char_before() {
        // "][" — cursor at 1 is on `[`, char before is `]`
        // Should match `[` first (char at cursor) since it's checked first
        let b = buf("][x]");
        let pair = find_matching_bracket(&b, 1).unwrap();
        assert_eq!(pair, BracketPair { open: 1, close: 3 });
    }

    // ── Multi-line ──────────────────────────────────────────────────

    #[test]
    fn match_across_lines() {
        let b = buf("(\n  hello\n)");
        let pair = find_matching_bracket(&b, 0).unwrap();
        assert_eq!(pair, BracketPair { open: 0, close: 10 });
    }

    // ── Bracket inside string (naive matching) ──────────────────────

    #[test]
    fn bracket_inside_string_matches_naively() {
        // Naive matching doesn't know about string literals
        let b = buf("(\")\")");
        // `(` at 0: walk forward sees `)` at 2 first — depth goes to 0
        let pair = find_matching_bracket(&b, 0).unwrap();
        assert_eq!(pair, BracketPair { open: 0, close: 2 });
    }
}
