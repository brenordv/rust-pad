/// Text buffer wrapping `ropey::Rope` for efficient text storage and manipulation.
use std::fmt;

use anyhow::Result;
use ropey::Rope;

/// A text buffer backed by a rope data structure for efficient editing.
#[derive(Debug, Clone)]
pub struct TextBuffer {
    rope: Rope,
}

impl Default for TextBuffer {
    fn default() -> Self {
        Self::new()
    }
}

impl From<&str> for TextBuffer {
    fn from(text: &str) -> Self {
        Self {
            rope: Rope::from_str(text),
        }
    }
}

impl fmt::Display for TextBuffer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.rope)
    }
}

impl TextBuffer {
    /// Creates an empty text buffer.
    pub fn new() -> Self {
        Self { rope: Rope::new() }
    }

    /// Returns the underlying rope (read-only).
    pub fn rope(&self) -> &Rope {
        &self.rope
    }

    /// Returns the total number of characters in the buffer.
    pub fn len_chars(&self) -> usize {
        self.rope.len_chars()
    }

    /// Returns the total number of bytes in the buffer.
    pub fn len_bytes(&self) -> usize {
        self.rope.len_bytes()
    }

    /// Returns the number of lines in the buffer.
    pub fn len_lines(&self) -> usize {
        self.rope.len_lines()
    }

    /// Returns true if the buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.rope.len_chars() == 0
    }

    /// Returns the text of a specific line (0-indexed), including any trailing newline.
    ///
    /// # Errors
    ///
    /// Returns an error if the line index is out of bounds.
    pub fn line(&self, line_idx: usize) -> Result<ropey::RopeSlice<'_>> {
        if line_idx >= self.rope.len_lines() {
            anyhow::bail!(
                "line index {} out of bounds (buffer has {} lines)",
                line_idx,
                self.rope.len_lines()
            );
        }
        Ok(self.rope.line(line_idx))
    }

    /// Returns the char index of the start of a line.
    ///
    /// # Errors
    ///
    /// Returns an error if the line index is out of bounds.
    pub fn line_to_char(&self, line_idx: usize) -> Result<usize> {
        if line_idx >= self.rope.len_lines() {
            anyhow::bail!(
                "line index {} out of bounds (buffer has {} lines)",
                line_idx,
                self.rope.len_lines()
            );
        }
        Ok(self.rope.line_to_char(line_idx))
    }

    /// Returns the line index for a given char index.
    ///
    /// # Errors
    ///
    /// Returns an error if the char index is out of bounds.
    pub fn char_to_line(&self, char_idx: usize) -> Result<usize> {
        if char_idx > self.rope.len_chars() {
            anyhow::bail!(
                "char index {} out of bounds (buffer has {} chars)",
                char_idx,
                self.rope.len_chars()
            );
        }
        Ok(self.rope.char_to_line(char_idx))
    }

    /// Returns the character at a given char index.
    ///
    /// # Errors
    ///
    /// Returns an error if the char index is out of bounds.
    pub fn char_at(&self, char_idx: usize) -> Result<char> {
        if char_idx >= self.rope.len_chars() {
            anyhow::bail!(
                "char index {} out of bounds (buffer has {} chars)",
                char_idx,
                self.rope.len_chars()
            );
        }
        Ok(self.rope.char(char_idx))
    }

    /// Inserts text at the given char index.
    ///
    /// # Errors
    ///
    /// Returns an error if the char index is out of bounds.
    pub fn insert(&mut self, char_idx: usize, text: &str) -> Result<()> {
        if char_idx > self.rope.len_chars() {
            anyhow::bail!(
                "insert position {} out of bounds (buffer has {} chars)",
                char_idx,
                self.rope.len_chars()
            );
        }
        self.rope.insert(char_idx, text);
        Ok(())
    }

    /// Removes the character range [start..end) from the buffer.
    ///
    /// # Errors
    ///
    /// Returns an error if the range is out of bounds.
    pub fn remove(&mut self, start: usize, end: usize) -> Result<()> {
        if start > end {
            anyhow::bail!("invalid range: start ({}) > end ({})", start, end);
        }
        if end > self.rope.len_chars() {
            anyhow::bail!(
                "range end {} out of bounds (buffer has {} chars)",
                end,
                self.rope.len_chars()
            );
        }
        self.rope.remove(start..end);
        Ok(())
    }

    /// Returns a slice of text in the given char range.
    ///
    /// # Errors
    ///
    /// Returns an error if the range is out of bounds.
    pub fn slice(&self, start: usize, end: usize) -> Result<ropey::RopeSlice<'_>> {
        if start > end {
            anyhow::bail!("invalid range: start ({}) > end ({})", start, end);
        }
        if end > self.rope.len_chars() {
            anyhow::bail!(
                "range end {} out of bounds (buffer has {} chars)",
                end,
                self.rope.len_chars()
            );
        }
        Ok(self.rope.slice(start..end))
    }

    /// Returns the length of a line in characters, excluding any trailing newline.
    ///
    /// # Errors
    ///
    /// Returns an error if the line index is out of bounds.
    pub fn line_len_chars(&self, line_idx: usize) -> Result<usize> {
        let line = self.line(line_idx)?;
        let len = line.len_chars();
        // Subtract trailing line ending if present (\n or \r\n)
        if len > 0 {
            let last_char = line.char(len - 1);
            if last_char == '\n' {
                // Also strip \r in \r\n (CRLF) sequences
                if len > 1 && line.char(len - 2) == '\r' {
                    return Ok(len - 2);
                }
                return Ok(len - 1);
            }
        }
        Ok(len)
    }

    /// Converts a byte offset to a char offset. O(log n) via ropey.
    ///
    /// # Errors
    ///
    /// Returns an error if the byte index is out of bounds.
    pub fn byte_to_char(&self, byte_idx: usize) -> Result<usize> {
        if byte_idx > self.rope.len_bytes() {
            anyhow::bail!(
                "byte index {} out of bounds (buffer has {} bytes)",
                byte_idx,
                self.rope.len_bytes()
            );
        }
        Ok(self.rope.byte_to_char(byte_idx))
    }

    /// Replaces text in the given char range with new text.
    ///
    /// # Errors
    ///
    /// Returns an error if the range is out of bounds.
    pub fn replace(&mut self, start: usize, end: usize, text: &str) -> Result<()> {
        self.remove(start, end)?;
        self.insert(start, text)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_buffer_is_empty() {
        let buf = TextBuffer::new();
        assert!(buf.is_empty());
        assert_eq!(buf.len_chars(), 0);
    }

    #[test]
    fn test_from_str() {
        let buf = TextBuffer::from("hello\nworld");
        assert_eq!(buf.len_chars(), 11);
        assert_eq!(buf.len_lines(), 2);
    }

    #[test]
    fn test_insert_and_remove() {
        let mut buf = TextBuffer::new();
        buf.insert(0, "hello").unwrap();
        assert_eq!(buf.to_string(), "hello");

        buf.insert(5, " world").unwrap();
        assert_eq!(buf.to_string(), "hello world");

        buf.remove(5, 11).unwrap();
        assert_eq!(buf.to_string(), "hello");
    }

    #[test]
    fn test_line_operations() {
        let buf = TextBuffer::from("line1\nline2\nline3");
        assert_eq!(buf.len_lines(), 3);

        let line = buf.line(0).unwrap();
        assert_eq!(line.to_string(), "line1\n");

        let line = buf.line(2).unwrap();
        assert_eq!(line.to_string(), "line3");
    }

    #[test]
    fn test_line_len_chars() {
        let buf = TextBuffer::from("hello\nworld");
        assert_eq!(buf.line_len_chars(0).unwrap(), 5);
        assert_eq!(buf.line_len_chars(1).unwrap(), 5);
    }

    #[test]
    fn test_char_to_line() {
        let buf = TextBuffer::from("abc\ndef\nghi");
        assert_eq!(buf.char_to_line(0).unwrap(), 0);
        assert_eq!(buf.char_to_line(4).unwrap(), 1);
        assert_eq!(buf.char_to_line(8).unwrap(), 2);
    }

    #[test]
    fn test_replace() {
        let mut buf = TextBuffer::from("hello world");
        buf.replace(0, 5, "goodbye").unwrap();
        assert_eq!(buf.to_string(), "goodbye world");
    }

    #[test]
    fn test_slice() {
        let buf = TextBuffer::from("hello world");
        let slice = buf.slice(0, 5).unwrap();
        assert_eq!(slice.to_string(), "hello");
    }

    #[test]
    fn test_out_of_bounds() {
        let buf = TextBuffer::from("hello");
        assert!(buf.line(5).is_err());
        assert!(buf.char_at(10).is_err());
        assert!(buf.slice(0, 100).is_err());
    }

    // ── Default and Display traits ───────────────────────────────────

    #[test]
    fn test_default_is_empty() {
        let buf = TextBuffer::default();
        assert!(buf.is_empty());
        assert_eq!(buf.len_chars(), 0);
        assert_eq!(buf.len_bytes(), 0);
        assert_eq!(buf.len_lines(), 1); // ropey always has at least 1 line
    }

    #[test]
    fn test_display_trait() {
        let buf = TextBuffer::from("hello\nworld");
        assert_eq!(buf.to_string(), "hello\nworld");
    }

    #[test]
    fn test_display_empty_buffer() {
        let buf = TextBuffer::new();
        assert_eq!(buf.to_string(), "");
    }

    // ── Unicode handling ─────────────────────────────────────────────

    #[test]
    fn test_unicode_multi_byte_chars() {
        let buf = TextBuffer::from("héllo 🌍");
        // h=1, é=1, l=1, l=1, o=1, ' '=1, 🌍=1 = 7 chars
        assert_eq!(buf.len_chars(), 7);
        assert!(buf.len_bytes() > 7); // multi-byte encoding
        assert_eq!(buf.char_at(1).unwrap(), 'é');
        assert_eq!(buf.char_at(6).unwrap(), '🌍');
    }

    #[test]
    fn test_unicode_insert_and_slice() {
        let mut buf = TextBuffer::from("abc");
        buf.insert(1, "日本語").unwrap();
        assert_eq!(buf.to_string(), "a日本語bc");
        assert_eq!(buf.len_chars(), 6);
        let slice = buf.slice(1, 4).unwrap();
        assert_eq!(slice.to_string(), "日本語");
    }

    #[test]
    fn test_unicode_remove() {
        let mut buf = TextBuffer::from("a🌍b🎉c");
        buf.remove(1, 2).unwrap(); // remove '🌍'
        assert_eq!(buf.to_string(), "ab🎉c");
    }

    #[test]
    fn test_unicode_line_operations() {
        let buf = TextBuffer::from("日本語\n中文\n한국어");
        assert_eq!(buf.len_lines(), 3);
        assert_eq!(buf.line(0).unwrap().to_string(), "日本語\n");
        assert_eq!(buf.line(1).unwrap().to_string(), "中文\n");
        assert_eq!(buf.line(2).unwrap().to_string(), "한국어");
    }

    // ── len_bytes ────────────────────────────────────────────────────

    #[test]
    fn test_len_bytes_ascii() {
        let buf = TextBuffer::from("hello");
        assert_eq!(buf.len_bytes(), 5);
    }

    #[test]
    fn test_len_bytes_multibyte() {
        let buf = TextBuffer::from("é"); // 2 bytes in UTF-8
        assert_eq!(buf.len_chars(), 1);
        assert_eq!(buf.len_bytes(), 2);
    }

    // ── Error paths ──────────────────────────────────────────────────

    #[test]
    fn test_insert_out_of_bounds() {
        let mut buf = TextBuffer::from("hello");
        assert!(buf.insert(100, "x").is_err());
    }

    #[test]
    fn test_remove_start_greater_than_end() {
        let mut buf = TextBuffer::from("hello");
        assert!(buf.remove(3, 1).is_err());
    }

    #[test]
    fn test_remove_end_out_of_bounds() {
        let mut buf = TextBuffer::from("hello");
        assert!(buf.remove(0, 100).is_err());
    }

    #[test]
    fn test_slice_start_greater_than_end() {
        let buf = TextBuffer::from("hello");
        assert!(buf.slice(3, 1).is_err());
    }

    #[test]
    fn test_line_to_char_out_of_bounds() {
        let buf = TextBuffer::from("hello");
        assert!(buf.line_to_char(5).is_err());
    }

    #[test]
    fn test_char_to_line_out_of_bounds() {
        let buf = TextBuffer::from("hello");
        assert!(buf.char_to_line(100).is_err());
    }

    #[test]
    fn test_line_out_of_bounds() {
        let buf = TextBuffer::from("hello\nworld");
        assert!(buf.line(2).is_err());
    }

    #[test]
    fn test_line_len_chars_out_of_bounds() {
        let buf = TextBuffer::from("hello");
        assert!(buf.line_len_chars(5).is_err());
    }

    #[test]
    fn test_replace_out_of_bounds() {
        let mut buf = TextBuffer::from("hello");
        assert!(buf.replace(0, 100, "x").is_err());
    }

    // ── byte_to_char ────────────────────────────────────────────────

    #[test]
    fn test_byte_to_char_ascii() {
        let buf = TextBuffer::from("hello");
        assert_eq!(buf.byte_to_char(0).unwrap(), 0);
        assert_eq!(buf.byte_to_char(3).unwrap(), 3);
        assert_eq!(buf.byte_to_char(5).unwrap(), 5);
    }

    #[test]
    fn test_byte_to_char_multibyte() {
        let buf = TextBuffer::from("héllo");
        // h=1 byte, é=2 bytes, l=1, l=1, o=1 → 6 bytes total
        assert_eq!(buf.byte_to_char(0).unwrap(), 0); // 'h'
        assert_eq!(buf.byte_to_char(1).unwrap(), 1); // start of 'é'
        assert_eq!(buf.byte_to_char(3).unwrap(), 2); // 'l' (after 2-byte 'é')
    }

    #[test]
    fn test_byte_to_char_out_of_bounds() {
        let buf = TextBuffer::from("hello");
        assert!(buf.byte_to_char(100).is_err());
    }

    // ── Empty buffer operations ──────────────────────────────────────

    #[test]
    fn test_empty_buffer_line_count() {
        let buf = TextBuffer::new();
        assert_eq!(buf.len_lines(), 1); // ropey gives 1 even for empty
    }

    #[test]
    fn test_insert_at_start_of_empty() {
        let mut buf = TextBuffer::new();
        buf.insert(0, "hello").unwrap();
        assert_eq!(buf.to_string(), "hello");
    }

    #[test]
    fn test_remove_empty_range() {
        let mut buf = TextBuffer::from("hello");
        buf.remove(2, 2).unwrap(); // empty range — no-op
        assert_eq!(buf.to_string(), "hello");
    }

    #[test]
    fn test_slice_empty_range() {
        let buf = TextBuffer::from("hello");
        let slice = buf.slice(2, 2).unwrap();
        assert_eq!(slice.to_string(), "");
    }

    // ── line_len_chars edge cases ────────────────────────────────────

    #[test]
    fn test_line_len_chars_with_crlf() {
        let buf = TextBuffer::from("hello\r\nworld");
        // After ropey normalization, line 0 is "hello\r\n"
        // line_len_chars strips both \r and \n
        assert_eq!(buf.line_len_chars(0).unwrap(), 5);
    }

    #[test]
    fn test_line_len_chars_last_line_no_newline() {
        let buf = TextBuffer::from("hello\nworld");
        // Last line has no trailing newline
        assert_eq!(buf.line_len_chars(1).unwrap(), 5);
    }

    #[test]
    fn test_line_len_chars_empty_line() {
        let buf = TextBuffer::from("hello\n\nworld");
        assert_eq!(buf.line_len_chars(1).unwrap(), 0);
    }

    // ── Cross-line operations ────────────────────────────────────────

    #[test]
    fn test_remove_across_lines() {
        let mut buf = TextBuffer::from("hello\nworld\nfoo");
        // h=0, e=1, l=2, l=3, o=4, \n=5, w=6, o=7, r=8, l=9, d=10, \n=11
        // Remove chars 3..9 -> removes "lo\nwor"
        buf.remove(3, 9).unwrap();
        assert_eq!(buf.to_string(), "helld\nfoo");
    }

    #[test]
    fn test_insert_newlines() {
        let mut buf = TextBuffer::from("helloworld");
        buf.insert(5, "\n").unwrap();
        assert_eq!(buf.to_string(), "hello\nworld");
        assert_eq!(buf.len_lines(), 2);
    }

    // ── rope() accessor ──────────────────────────────────────────────

    #[test]
    fn test_rope_accessor() {
        let buf = TextBuffer::from("hello");
        let rope = buf.rope();
        assert_eq!(rope.len_chars(), 5);
    }

    // ── char_at and char_to_line ─────────────────────────────────────

    #[test]
    fn test_char_at_various_positions() {
        let buf = TextBuffer::from("abc\ndef");
        assert_eq!(buf.char_at(0).unwrap(), 'a');
        assert_eq!(buf.char_at(2).unwrap(), 'c');
        assert_eq!(buf.char_at(3).unwrap(), '\n');
        assert_eq!(buf.char_at(4).unwrap(), 'd');
    }

    #[test]
    fn test_char_to_line_at_newline() {
        let buf = TextBuffer::from("abc\ndef\nghi");
        // The \n at index 3 belongs to line 0
        assert_eq!(buf.char_to_line(3).unwrap(), 0);
        // The 'd' at index 4 belongs to line 1
        assert_eq!(buf.char_to_line(4).unwrap(), 1);
    }

    #[test]
    fn test_char_to_line_at_end() {
        let buf = TextBuffer::from("abc\ndef");
        // char_to_line allows char_idx == len_chars (past-the-end)
        assert_eq!(buf.char_to_line(7).unwrap(), 1);
    }

    #[test]
    fn test_line_to_char_all_lines() {
        let buf = TextBuffer::from("abc\ndef\nghi");
        assert_eq!(buf.line_to_char(0).unwrap(), 0);
        assert_eq!(buf.line_to_char(1).unwrap(), 4);
        assert_eq!(buf.line_to_char(2).unwrap(), 8);
    }
}
