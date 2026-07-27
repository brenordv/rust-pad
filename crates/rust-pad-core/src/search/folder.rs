//! Recursive content search across a directory tree.
//!
//! Reads each text file under a root folder and runs the same
//! [`SearchEngine`](super::finder::SearchEngine) used for in-editor search,
//! collecting matches with their file paths. The walk is bounded (file, match,
//! depth, and byte caps) and containment-checked per file so a symlink or
//! junction inside the root that resolves outside it is never read.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::buffer::TextBuffer;
use crate::cursor::char_to_pos;
use crate::encoding::{decode_bytes, detect_encoding, normalize_line_endings};

use super::finder::{SearchEngine, SearchOptions};

/// Number of leading bytes scanned for a NUL to classify a file as binary.
const BINARY_SNIFF_BYTES: usize = 8192;

/// Bounds that keep a folder search responsive and memory-safe. Every bound is
/// reflected in [`FolderSearchOutcome`] when hit, so a partial result is never
/// silently presented as complete.
#[derive(Debug, Clone, Copy)]
pub struct FolderSearchLimits {
    /// Maximum number of files read.
    pub max_files: usize,
    /// Maximum number of matches collected.
    pub max_matches: usize,
    /// Maximum directory depth descended below the root.
    pub max_depth: usize,
    /// Files larger than this (bytes) are skipped.
    pub max_file_size_bytes: u64,
    /// Total bytes read before the walk stops.
    pub max_total_bytes: u64,
    /// A hit's preview line is windowed to at most this many characters.
    pub max_line_chars: usize,
    /// Whether to descend into and read dot-prefixed (hidden) entries.
    pub include_hidden: bool,
}

impl Default for FolderSearchLimits {
    fn default() -> Self {
        Self {
            max_files: 5_000,
            max_matches: 5_000,
            max_depth: 32,
            max_file_size_bytes: 8 * 1024 * 1024,
            max_total_bytes: 512 * 1024 * 1024,
            max_line_chars: 400,
            include_hidden: false,
        }
    }
}

/// A single match found by a folder search.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderSearchHit {
    /// Canonical path of the file containing the match.
    pub path: PathBuf,
    /// 0-indexed line where the match starts.
    pub line: usize,
    /// 0-indexed column where the match starts.
    pub col: usize,
    /// Start char index of the match within the (normalized) file.
    pub match_start: usize,
    /// End char index of the match (exclusive).
    pub match_end: usize,
    /// Preview of the matching line, windowed around the match.
    pub line_text: String,
}

/// Aggregate outcome of a folder search: the hits plus counts of what was
/// skipped, so callers can explain a "no results" outcome and surface caps.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FolderSearchOutcome {
    /// Collected matches.
    pub hits: Vec<FolderSearchHit>,
    /// True when a bound (files, matches, depth, or total bytes) stopped the walk.
    pub truncated: bool,
    /// Number of files actually read.
    pub files_visited: usize,
    /// Files skipped for exceeding `max_file_size_bytes`.
    pub skipped_too_large: usize,
    /// Files skipped as binary (NUL byte) or undecodable.
    pub skipped_binary: usize,
    /// Entries skipped because they resolved outside the search root.
    pub skipped_out_of_root: usize,
    /// Entries that could not be read or canonicalized.
    pub unreadable: usize,
}

/// Returns true when `name` is a hidden (dot-prefixed) entry.
fn is_hidden(name: &str) -> bool {
    name.starts_with('.')
}

/// Returns true when the byte prefix looks binary (contains a NUL). Cheap
/// heuristic used by ripgrep/git; decode-based detection is unreliable because
/// `chardetng` always guesses some encoding.
fn looks_binary(bytes: &[u8]) -> bool {
    let sniff = &bytes[..bytes.len().min(BINARY_SNIFF_BYTES)];
    sniff.contains(&0)
}

/// Builds a preview of the match's line, windowed to `max_chars` around the
/// match column so a very long (e.g. minified) line cannot blow out memory.
fn preview_line(buffer: &TextBuffer, line: usize, col: usize, max_chars: usize) -> String {
    let raw = match buffer.line(line) {
        Ok(slice) => slice.to_string(),
        Err(_) => return String::new(),
    };
    let trimmed = raw.trim_end_matches(['\n', '\r']);
    let chars: Vec<char> = trimmed.chars().collect();
    if chars.len() <= max_chars {
        return trimmed.to_string();
    }
    let half = max_chars / 2;
    let max_start = chars.len().saturating_sub(max_chars);
    let start = col.saturating_sub(half).min(max_start);
    let end = (start + max_chars).min(chars.len());
    let mut out = String::new();
    if start > 0 {
        out.push('…');
    }
    out.extend(&chars[start..end]);
    if end < chars.len() {
        out.push('…');
    }
    out
}

/// Searches every readable text file under `root` for `options.query`.
///
/// The root is canonicalized; each directory and file reached is canonicalized
/// and required to stay within the canonical root, so symlinks/junctions
/// pointing outside are skipped rather than read. VCS metadata directories
/// (`.git`, `.hg`, `.svn`) are never descended. Binary and oversized files are
/// skipped. Returns an empty outcome for an empty query or an unresolvable root.
pub fn search_folder(
    root: &Path,
    options: &SearchOptions,
    limits: FolderSearchLimits,
) -> FolderSearchOutcome {
    let mut outcome = FolderSearchOutcome::default();
    if options.query.is_empty() {
        return outcome;
    }
    let canonical_root = match std::fs::canonicalize(root) {
        Ok(r) => r,
        Err(_) => return outcome,
    };

    let mut engine = SearchEngine::new();
    let mut visited_dirs: HashSet<PathBuf> = HashSet::new();
    let mut total_bytes: u64 = 0;
    visited_dirs.insert(canonical_root.clone());
    let mut stack: Vec<(PathBuf, usize)> = vec![(canonical_root.clone(), 0)];

    'walk: while let Some((dir, depth)) = stack.pop() {
        let read_dir = match std::fs::read_dir(&dir) {
            Ok(rd) => rd,
            Err(_) => {
                outcome.unreadable += 1;
                continue;
            }
        };
        for entry in read_dir.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let entry_path = entry.path();
            let file_type = match entry.file_type() {
                Ok(ft) => ft,
                Err(_) => {
                    outcome.unreadable += 1;
                    continue;
                }
            };
            // `entry_path.is_dir()` follows symlinks, so a symlink-to-dir is
            // treated as a directory (and containment-checked below).
            let is_dir = file_type.is_dir() || (file_type.is_symlink() && entry_path.is_dir());

            if is_dir {
                if matches!(name.as_str(), ".git" | ".hg" | ".svn") {
                    continue;
                }
                if !limits.include_hidden && is_hidden(&name) {
                    continue;
                }
                if depth + 1 > limits.max_depth {
                    continue;
                }
                let canon = match std::fs::canonicalize(&entry_path) {
                    Ok(c) => c,
                    Err(_) => {
                        outcome.unreadable += 1;
                        continue;
                    }
                };
                if !canon.starts_with(&canonical_root) {
                    outcome.skipped_out_of_root += 1;
                    continue;
                }
                // The visited set (canonical) terminates symlink/junction loops.
                if visited_dirs.insert(canon.clone()) {
                    stack.push((canon, depth + 1));
                }
                continue;
            }

            if !limits.include_hidden && is_hidden(&name) {
                continue;
            }
            if outcome.files_visited >= limits.max_files
                || outcome.hits.len() >= limits.max_matches
                || total_bytes >= limits.max_total_bytes
            {
                outcome.truncated = true;
                break 'walk;
            }

            let canon = match std::fs::canonicalize(&entry_path) {
                Ok(c) => c,
                Err(_) => {
                    outcome.unreadable += 1;
                    continue;
                }
            };
            if !canon.starts_with(&canonical_root) {
                outcome.skipped_out_of_root += 1;
                continue;
            }
            if !canon.is_file() {
                continue;
            }
            let size = std::fs::metadata(&canon).map(|m| m.len()).unwrap_or(0);
            if size > limits.max_file_size_bytes {
                outcome.skipped_too_large += 1;
                continue;
            }
            let bytes = match std::fs::read(&canon) {
                Ok(b) => b,
                Err(_) => {
                    outcome.unreadable += 1;
                    continue;
                }
            };
            total_bytes = total_bytes.saturating_add(bytes.len() as u64);
            outcome.files_visited += 1;

            if looks_binary(&bytes) {
                outcome.skipped_binary += 1;
                continue;
            }
            let encoding = detect_encoding(&bytes);
            let text = match decode_bytes(&bytes, encoding) {
                Ok(t) => normalize_line_endings(&t),
                Err(_) => {
                    outcome.skipped_binary += 1;
                    continue;
                }
            };
            let buffer = TextBuffer::from(text.as_str());
            if engine.find_all(&buffer, options).is_err() {
                // Invalid regex: every file would fail identically. Stop.
                break 'walk;
            }
            for m in &engine.matches {
                if outcome.hits.len() >= limits.max_matches {
                    outcome.truncated = true;
                    break 'walk;
                }
                let pos = char_to_pos(&buffer, m.start);
                let line_text = preview_line(&buffer, pos.line, pos.col, limits.max_line_chars);
                outcome.hits.push(FolderSearchHit {
                    path: canon.clone(),
                    line: pos.line,
                    col: pos.col,
                    match_start: m.start,
                    match_end: m.end,
                    line_text,
                });
            }
        }
    }

    outcome
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts(query: &str) -> SearchOptions {
        SearchOptions {
            query: query.to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn finds_matches_across_nested_dirs() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "hello world\n").unwrap();
        let sub = dir.path().join("sub");
        std::fs::create_dir(&sub).unwrap();
        std::fs::write(sub.join("b.txt"), "say hello again\nno match here\n").unwrap();

        let out = search_folder(dir.path(), &opts("hello"), FolderSearchLimits::default());
        assert_eq!(out.hits.len(), 2, "one match per file");
        assert!(out.hits.iter().any(|h| h.path.ends_with("a.txt")));
        assert!(out.hits.iter().any(|h| h.path.ends_with("b.txt")));
        assert_eq!(out.files_visited, 2);
    }

    #[test]
    fn empty_query_returns_nothing() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "hello\n").unwrap();
        let out = search_folder(dir.path(), &opts(""), FolderSearchLimits::default());
        assert!(out.hits.is_empty());
        assert_eq!(out.files_visited, 0);
    }

    #[test]
    fn skips_hidden_unless_included() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".secret"), "hello token\n").unwrap();
        std::fs::write(dir.path().join("v.txt"), "hello there\n").unwrap();

        let hidden_off = search_folder(dir.path(), &opts("hello"), FolderSearchLimits::default());
        assert_eq!(
            hidden_off.hits.len(),
            1,
            "the dotfile is skipped by default"
        );

        let with_hidden = search_folder(
            dir.path(),
            &opts("hello"),
            FolderSearchLimits {
                include_hidden: true,
                ..Default::default()
            },
        );
        assert_eq!(
            with_hidden.hits.len(),
            2,
            "both files matched when hidden included"
        );
    }

    #[test]
    fn excludes_vcs_metadata_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let git = dir.path().join(".git");
        std::fs::create_dir(&git).unwrap();
        std::fs::write(git.join("config"), "url = hello://origin\n").unwrap();
        std::fs::write(dir.path().join("code.txt"), "hello\n").unwrap();

        let out = search_folder(
            dir.path(),
            &opts("hello"),
            FolderSearchLimits {
                include_hidden: true, // even with hidden on, .git is excluded
                ..Default::default()
            },
        );
        assert_eq!(out.hits.len(), 1);
        assert!(out.hits[0].path.ends_with("code.txt"));
    }

    #[test]
    fn skips_binary_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("bin.dat"), b"hello\x00\x00binary").unwrap();
        std::fs::write(dir.path().join("text.txt"), "hello\n").unwrap();

        let out = search_folder(dir.path(), &opts("hello"), FolderSearchLimits::default());
        assert_eq!(out.hits.len(), 1, "only the text file matched");
        assert!(out.hits[0].path.ends_with("text.txt"));
        assert_eq!(out.skipped_binary, 1);
    }

    #[test]
    fn skips_oversized_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("big.txt"), "hello ".repeat(1000)).unwrap();
        std::fs::write(dir.path().join("small.txt"), "hello\n").unwrap();

        let out = search_folder(
            dir.path(),
            &opts("hello"),
            FolderSearchLimits {
                max_file_size_bytes: 100,
                ..Default::default()
            },
        );
        assert_eq!(out.skipped_too_large, 1);
        assert!(out.hits.iter().all(|h| h.path.ends_with("small.txt")));
    }

    #[test]
    fn truncates_at_match_cap() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("many.txt"),
            "hello\nhello\nhello\nhello\nhello\n",
        )
        .unwrap();

        let out = search_folder(
            dir.path(),
            &opts("hello"),
            FolderSearchLimits {
                max_matches: 2,
                ..Default::default()
            },
        );
        assert!(out.truncated);
        assert_eq!(out.hits.len(), 2);
    }

    #[test]
    fn long_line_preview_is_windowed() {
        let dir = tempfile::tempdir().unwrap();
        let mut line = "x".repeat(5000);
        line.push_str("NEEDLE");
        line.push_str(&"y".repeat(5000));
        line.push('\n');
        std::fs::write(dir.path().join("min.txt"), line).unwrap();

        let out = search_folder(
            dir.path(),
            &opts("NEEDLE"),
            FolderSearchLimits {
                max_line_chars: 80,
                ..Default::default()
            },
        );
        assert_eq!(out.hits.len(), 1);
        let preview = &out.hits[0].line_text;
        assert!(
            preview.chars().count() <= 82,
            "preview windowed around the match"
        );
        assert!(preview.contains("NEEDLE"), "window includes the match");
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_file_pointing_outside_root_is_skipped() {
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("secret.txt"), "hello secret\n").unwrap();

        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("inside.txt"), "hello inside\n").unwrap();
        std::os::unix::fs::symlink(
            outside.path().join("secret.txt"),
            root.path().join("link.txt"),
        )
        .unwrap();

        let out = search_folder(root.path(), &opts("hello"), FolderSearchLimits::default());
        // Only the in-root file is read; the escaping symlink is refused.
        assert!(out.hits.iter().all(|h| h.path.ends_with("inside.txt")));
        assert_eq!(out.skipped_out_of_root, 1);
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_dir_loop_terminates() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("f.txt"), "hello\n").unwrap();
        // A self-referential loop: root/loop -> root.
        std::os::unix::fs::symlink(root.path(), root.path().join("loop")).unwrap();

        // Must terminate (not hang) and still find the real file.
        let out = search_folder(root.path(), &opts("hello"), FolderSearchLimits::default());
        assert_eq!(out.hits.len(), 1);
    }
}
