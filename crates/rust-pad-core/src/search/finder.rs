/// Search engine supporting plain text and regex search with match highlighting.
use anyhow::{Context, Result};
use regex::Regex;

use crate::buffer::TextBuffer;

/// A single search match in the buffer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchMatch {
    /// Start char index in the buffer.
    pub start: usize,
    /// End char index in the buffer (exclusive).
    pub end: usize,
    /// 0-indexed line number where the match starts.
    pub line: usize,
}

/// Search configuration options.
#[derive(Debug, Clone, Default)]
pub struct SearchOptions {
    /// The search query string.
    pub query: String,
    /// Whether to use regex search.
    pub use_regex: bool,
    /// Whether search is case-sensitive.
    pub case_sensitive: bool,
    /// Whether to match whole words only.
    pub whole_word: bool,
}

/// The search engine for finding text in a buffer.
#[derive(Debug)]
pub struct SearchEngine {
    /// Compiled regex pattern (cached).
    compiled: Option<Regex>,
    /// The options used to compile the current regex.
    compiled_for: Option<String>,
    /// All matches found.
    pub matches: Vec<SearchMatch>,
    /// Index of the current/active match.
    pub current_match: Option<usize>,
    /// Content version when matches were last computed (for cache invalidation).
    last_search_version: Option<u64>,
    /// Cache key combining query + options for the last search.
    last_search_key: Option<String>,
}

impl Default for SearchEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl SearchEngine {
    /// Creates a new search engine.
    pub fn new() -> Self {
        Self {
            compiled: None,
            compiled_for: None,
            matches: Vec::new(),
            current_match: None,
            last_search_version: None,
            last_search_key: None,
        }
    }

    /// Builds a regex pattern from the search options.
    fn build_pattern(options: &SearchOptions) -> Result<Regex> {
        let mut pattern = if options.use_regex {
            options.query.clone()
        } else {
            regex::escape(&options.query)
        };

        if options.whole_word {
            pattern = format!(r"\b{pattern}\b");
        }

        let regex = if options.case_sensitive {
            Regex::new(&pattern)
        } else {
            Regex::new(&format!("(?i){pattern}"))
        };

        regex.context("invalid search pattern")
    }

    /// Finds all matches in the buffer.
    ///
    /// # Errors
    ///
    /// Returns an error if the regex pattern is invalid.
    pub fn find_all(&mut self, buffer: &TextBuffer, options: &SearchOptions) -> Result<()> {
        self.find_all_versioned(buffer, options, None)
    }

    /// Finds all matches in the buffer, with optional version-based caching.
    ///
    /// When `content_version` is provided, skips re-searching if the version
    /// and query options haven't changed since the last call.
    ///
    /// # Errors
    ///
    /// Returns an error if the regex pattern is invalid.
    pub fn find_all_versioned(
        &mut self,
        buffer: &TextBuffer,
        options: &SearchOptions,
        content_version: Option<u64>,
    ) -> Result<()> {
        if options.query.is_empty() {
            self.matches.clear();
            self.current_match = None;
            self.last_search_version = None;
            self.last_search_key = None;
            return Ok(());
        }

        // Build/cache regex
        let cache_key = format!(
            "{}:{}:{}:{}",
            options.query, options.use_regex, options.case_sensitive, options.whole_word
        );

        // Check if we can reuse cached results
        if let Some(version) = content_version {
            if self.last_search_version == Some(version)
                && self.last_search_key.as_deref() == Some(&cache_key)
            {
                return Ok(());
            }
        }

        if self.compiled_for.as_deref() != Some(&cache_key) {
            self.compiled = Some(Self::build_pattern(options)?);
            self.compiled_for = Some(cache_key.clone());
        }

        let regex = match &self.compiled {
            Some(r) => r,
            None => return Ok(()),
        };

        self.matches.clear();
        self.current_match = None;

        let text = buffer.to_string();

        // Find all byte-offset matches, then convert to char offsets using ropey's O(log n) method
        for mat in regex.find_iter(&text) {
            let byte_start = mat.start();
            let byte_end = mat.end();

            let char_start = buffer.byte_to_char(byte_start).unwrap_or(0);
            let char_end = buffer.byte_to_char(byte_end).unwrap_or(char_start);

            let line = buffer.char_to_line(char_start).unwrap_or(0);

            self.matches.push(SearchMatch {
                start: char_start,
                end: char_end,
                line,
            });
        }

        if !self.matches.is_empty() {
            self.current_match = Some(0);
        }

        // Store cache key
        self.last_search_version = content_version;
        self.last_search_key = Some(cache_key);

        Ok(())
    }

    /// Moves to the next match at or after the given cursor position.
    /// Returns the match index.
    pub fn find_next(&mut self, cursor_char_idx: usize) -> Option<usize> {
        if self.matches.is_empty() {
            return None;
        }

        // Find the first match that starts at or after the cursor position.
        // Using >= because after selecting a match the cursor sits at mat.end,
        // and the very next match may start at that same char offset.
        let idx = self
            .matches
            .iter()
            .position(|m| m.start >= cursor_char_idx)
            .unwrap_or(0); // Wrap around to first match

        self.current_match = Some(idx);
        Some(idx)
    }

    /// Moves to the previous match before the given cursor position.
    /// Returns the match index.
    pub fn find_prev(&mut self, cursor_char_idx: usize) -> Option<usize> {
        if self.matches.is_empty() {
            return None;
        }

        // Find the last match that starts before the cursor
        let idx = self
            .matches
            .iter()
            .rposition(|m| m.start < cursor_char_idx)
            .unwrap_or(self.matches.len() - 1); // Wrap around to last match

        self.current_match = Some(idx);
        Some(idx)
    }

    /// Replaces the current match with the replacement text.
    /// Returns true if a replacement was made.
    pub fn replace_current(
        &mut self,
        buffer: &mut TextBuffer,
        replacement: &str,
        options: &SearchOptions,
    ) -> Result<bool> {
        let idx = match self.current_match {
            Some(idx) if idx < self.matches.len() => idx,
            _ => return Ok(false),
        };

        let mat = &self.matches[idx];
        let start = mat.start;
        let end = mat.end;

        let actual_replacement = if options.use_regex {
            if let Some(ref regex) = self.compiled {
                let matched_text = buffer.slice(start, end)?.to_string();
                regex.replace(&matched_text, replacement).into_owned()
            } else {
                replacement.to_string()
            }
        } else {
            replacement.to_string()
        };

        buffer
            .replace(start, end, &actual_replacement)
            .context("failed to replace match")?;

        // Re-search to update match positions
        self.find_all(buffer, options)?;

        Ok(true)
    }

    /// Replaces all matches with the replacement text.
    /// Returns the number of replacements made.
    pub fn replace_all(
        &mut self,
        buffer: &mut TextBuffer,
        replacement: &str,
        options: &SearchOptions,
    ) -> Result<usize> {
        if self.matches.is_empty() {
            return Ok(0);
        }

        // Replace from end to start to preserve positions
        let count = self.matches.len();
        for mat in self.matches.iter().rev() {
            let actual_replacement = if options.use_regex {
                if let Some(ref regex) = self.compiled {
                    let matched_text = buffer.slice(mat.start, mat.end)?.to_string();
                    regex.replace(&matched_text, replacement).into_owned()
                } else {
                    replacement.to_string()
                }
            } else {
                replacement.to_string()
            };

            buffer.replace(mat.start, mat.end, &actual_replacement)?;
        }

        self.matches.clear();
        self.current_match = None;

        Ok(count)
    }

    /// Returns the total number of matches.
    pub fn match_count(&self) -> usize {
        self.matches.len()
    }

    /// Clears all search state.
    pub fn clear(&mut self) {
        self.matches.clear();
        self.current_match = None;
        self.compiled = None;
        self.compiled_for = None;
        self.last_search_version = None;
        self.last_search_key = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_all_plain() {
        let buf = TextBuffer::from("hello world hello");
        let mut engine = SearchEngine::new();
        let opts = SearchOptions {
            query: "hello".to_string(),
            ..Default::default()
        };
        engine.find_all(&buf, &opts).unwrap();
        assert_eq!(engine.match_count(), 2);
        assert_eq!(engine.matches[0].start, 0);
        assert_eq!(engine.matches[0].end, 5);
        assert_eq!(engine.matches[1].start, 12);
        assert_eq!(engine.matches[1].end, 17);
    }

    #[test]
    fn test_find_all_case_insensitive() {
        let buf = TextBuffer::from("Hello HELLO hello");
        let mut engine = SearchEngine::new();
        let opts = SearchOptions {
            query: "hello".to_string(),
            case_sensitive: false,
            ..Default::default()
        };
        engine.find_all(&buf, &opts).unwrap();
        assert_eq!(engine.match_count(), 3);
    }

    #[test]
    fn test_find_all_case_sensitive() {
        let buf = TextBuffer::from("Hello HELLO hello");
        let mut engine = SearchEngine::new();
        let opts = SearchOptions {
            query: "hello".to_string(),
            case_sensitive: true,
            ..Default::default()
        };
        engine.find_all(&buf, &opts).unwrap();
        assert_eq!(engine.match_count(), 1);
        assert_eq!(engine.matches[0].start, 12);
    }

    #[test]
    fn test_find_all_regex() {
        let buf = TextBuffer::from("foo123 bar456 baz");
        let mut engine = SearchEngine::new();
        let opts = SearchOptions {
            query: r"\d+".to_string(),
            use_regex: true,
            ..Default::default()
        };
        engine.find_all(&buf, &opts).unwrap();
        assert_eq!(engine.match_count(), 2);
    }

    #[test]
    fn test_find_all_whole_word() {
        let buf = TextBuffer::from("hello helloworld hello");
        let mut engine = SearchEngine::new();
        let opts = SearchOptions {
            query: "hello".to_string(),
            whole_word: true,
            ..Default::default()
        };
        engine.find_all(&buf, &opts).unwrap();
        assert_eq!(engine.match_count(), 2);
    }

    #[test]
    fn test_find_next_prev() {
        let buf = TextBuffer::from("aaa bbb aaa bbb aaa");
        let mut engine = SearchEngine::new();
        let opts = SearchOptions {
            query: "aaa".to_string(),
            ..Default::default()
        };
        engine.find_all(&buf, &opts).unwrap();
        assert_eq!(engine.match_count(), 3);
        // Matches at char positions: 0, 8, 16

        // Find next from position 0 — match at 0 starts at cursor, should find it
        let idx = engine.find_next(0).unwrap();
        assert_eq!(idx, 0);
        assert_eq!(engine.matches[idx].start, 0);

        // Find next from position 3 (end of first match) — next match at 8
        let idx = engine.find_next(3).unwrap();
        assert_eq!(idx, 1);
        assert_eq!(engine.matches[idx].start, 8);

        // Find prev from position 16 — previous match at 8
        let idx = engine.find_prev(16).unwrap();
        assert_eq!(idx, 1);
        assert_eq!(engine.matches[idx].start, 8);

        // Find prev from position 8 — previous match at 0
        let idx = engine.find_prev(8).unwrap();
        assert_eq!(idx, 0);
        assert_eq!(engine.matches[idx].start, 0);
    }

    #[test]
    fn test_replace_current() {
        let mut buf = TextBuffer::from("hello world hello");
        let mut engine = SearchEngine::new();
        let opts = SearchOptions {
            query: "hello".to_string(),
            ..Default::default()
        };
        engine.find_all(&buf, &opts).unwrap();
        engine.replace_current(&mut buf, "hi", &opts).unwrap();
        assert_eq!(buf.to_string(), "hi world hello");
    }

    #[test]
    fn test_replace_all() {
        let mut buf = TextBuffer::from("hello world hello");
        let mut engine = SearchEngine::new();
        let opts = SearchOptions {
            query: "hello".to_string(),
            ..Default::default()
        };
        engine.find_all(&buf, &opts).unwrap();
        let count = engine.replace_all(&mut buf, "hi", &opts).unwrap();
        assert_eq!(count, 2);
        assert_eq!(buf.to_string(), "hi world hi");
    }

    #[test]
    fn test_empty_query() {
        let buf = TextBuffer::from("hello");
        let mut engine = SearchEngine::new();
        let opts = SearchOptions::default();
        engine.find_all(&buf, &opts).unwrap();
        assert_eq!(engine.match_count(), 0);
    }

    #[test]
    fn test_multiline_search() {
        let buf = TextBuffer::from("line1\nline2\nline1");
        let mut engine = SearchEngine::new();
        let opts = SearchOptions {
            query: "line1".to_string(),
            ..Default::default()
        };
        engine.find_all(&buf, &opts).unwrap();
        assert_eq!(engine.match_count(), 2);
        assert_eq!(engine.matches[0].line, 0);
        assert_eq!(engine.matches[1].line, 2);
    }

    /// Regression: find_next must not skip adjacent/consecutive matches.
    /// Simulates the user repeatedly pressing "Find Next" starting from
    /// the end of the previous match (as the App does after selecting).
    #[test]
    fn test_find_next_visits_all_adjacent_matches() {
        let buf = TextBuffer::from("asdasdasdasd\nasdasd\nasd\nasd\nasd\nas\ndasd\nasd");
        let mut engine = SearchEngine::new();
        let opts = SearchOptions {
            query: "asd".to_string(),
            ..Default::default()
        };
        engine.find_all(&buf, &opts).unwrap();

        let total = engine.match_count();
        assert!(total >= 2, "need at least 2 matches");

        // Walk through every match by feeding mat.end as the next cursor pos
        let mut cursor = 0usize;
        let mut visited = Vec::new();
        for _ in 0..total {
            let idx = engine.find_next(cursor).unwrap();
            let mat = &engine.matches[idx];
            visited.push(idx);
            cursor = mat.end; // App moves cursor to end of selected match
        }

        // Every match index 0..total should appear exactly once, in order
        let expected: Vec<usize> = (0..total).collect();
        assert_eq!(
            visited, expected,
            "find_next must visit every match sequentially without skipping"
        );
    }

    /// Regression: find_prev must not get stuck re-finding the same match.
    /// Simulates the user pressing "Find Prev" from the start of the current match.
    #[test]
    fn test_find_prev_visits_all_matches_backward() {
        let buf = TextBuffer::from("asd xxx asd xxx asd");
        let mut engine = SearchEngine::new();
        let opts = SearchOptions {
            query: "asd".to_string(),
            ..Default::default()
        };
        engine.find_all(&buf, &opts).unwrap();

        let total = engine.match_count();
        assert_eq!(total, 3);

        // Start from the last match's start position (simulating selection start)
        let mut cursor = engine.matches[total - 1].start;
        let mut visited = Vec::new();
        for _ in 0..total {
            let idx = engine.find_prev(cursor).unwrap();
            let mat = &engine.matches[idx];
            visited.push(idx);
            cursor = mat.start; // FindPrev uses selection start
        }

        // Should visit 2, 1, 0 — or wrap around
        // From start=16, prev < 16 => idx 1 (start=8)
        // From start=8,  prev < 8  => idx 0 (start=0)
        // From start=0,  prev < 0  => wraps to idx 2 (last)
        assert_eq!(visited, vec![1, 0, 2]);
    }

    /// Regression: find_next with adjacent matches on separate lines.
    #[test]
    fn test_find_next_multiline_adjacent() {
        let buf = TextBuffer::from("asd\nasd\nasd");
        let mut engine = SearchEngine::new();
        let opts = SearchOptions {
            query: "asd".to_string(),
            ..Default::default()
        };
        engine.find_all(&buf, &opts).unwrap();
        assert_eq!(engine.match_count(), 3);

        // Match 0: chars 0..3, Match 1: chars 4..7, Match 2: chars 8..11
        // Walk with cursor = mat.end each time
        let idx0 = engine.find_next(0).unwrap();
        assert_eq!(idx0, 0);
        let idx1 = engine.find_next(engine.matches[idx0].end).unwrap();
        assert_eq!(idx1, 1);
        let idx2 = engine.find_next(engine.matches[idx1].end).unwrap();
        assert_eq!(idx2, 2);
        // After last match, should wrap to 0
        let idx_wrap = engine.find_next(engine.matches[idx2].end).unwrap();
        assert_eq!(idx_wrap, 0);
    }
}
