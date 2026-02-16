/// Configuration and utility functions for the history system.
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// Maximum number of edit groups kept in the in-memory hot cache
/// before spilling to disk.
const DEFAULT_HOT_CAPACITY: usize = 500;

/// Maximum total number of edit groups per document (memory + disk).
/// Oldest groups are evicted when this limit is exceeded.
const DEFAULT_MAX_HISTORY_DEPTH: usize = 10_000;

/// Time window in milliseconds for grouping consecutive edits
/// into a single undo step.
const DEFAULT_GROUP_TIMEOUT_MS: u64 = 500;

/// Configuration for the history system.
#[derive(Debug, Clone)]
pub struct HistoryConfig {
    /// Max edit groups in the hot (in-memory) cache.
    pub hot_capacity: usize,
    /// Max total edit groups per document (memory + disk).
    pub max_history_depth: usize,
    /// Grouping timeout in milliseconds.
    pub group_timeout_ms: u64,
    /// Root directory for the persistence database.
    pub data_dir: PathBuf,
}

impl Default for HistoryConfig {
    fn default() -> Self {
        Self {
            hot_capacity: DEFAULT_HOT_CAPACITY,
            max_history_depth: DEFAULT_MAX_HISTORY_DEPTH,
            group_timeout_ms: DEFAULT_GROUP_TIMEOUT_MS,
            data_dir: resolve_data_dir(),
        }
    }
}

/// Resolves the data directory path.
///
/// Resolution order:
/// 1. `RUST_PAD_DATA_DIR` environment variable
/// 2. `.data/` directory next to the executable
pub fn resolve_data_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("RUST_PAD_DATA_DIR") {
        return PathBuf::from(dir);
    }
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("."));
    exe.parent().unwrap_or(Path::new(".")).join(".data")
}

/// Generates a document ID for a file on disk.
///
/// Uses a hash of the canonical path for stability across sessions.
pub fn doc_id_for_path(path: &Path) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let canonical = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let mut hasher = DefaultHasher::new();
    canonical.hash(&mut hasher);
    format!("file-{:016x}", hasher.finish())
}

/// Counter for generating unique unsaved document IDs within a session.
static UNSAVED_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Generates a unique document ID for an unsaved (new) document.
pub fn generate_unsaved_id() -> String {
    let count = UNSAVED_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("unsaved-{count}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = HistoryConfig::default();
        assert_eq!(config.hot_capacity, 500);
        assert_eq!(config.max_history_depth, 10_000);
        assert_eq!(config.group_timeout_ms, 500);
    }

    #[test]
    fn test_generate_unsaved_ids_are_unique() {
        let id1 = generate_unsaved_id();
        let id2 = generate_unsaved_id();
        assert_ne!(id1, id2);
        assert!(id1.starts_with("unsaved-"));
        assert!(id2.starts_with("unsaved-"));
    }

    #[test]
    fn test_doc_id_for_path_consistent() {
        let path = PathBuf::from("test_file.txt");
        let id1 = doc_id_for_path(&path);
        let id2 = doc_id_for_path(&path);
        assert_eq!(id1, id2);
        assert!(id1.starts_with("file-"));
    }

    #[test]
    fn test_doc_id_for_different_paths_differ() {
        let id1 = doc_id_for_path(Path::new("file_a.txt"));
        let id2 = doc_id_for_path(Path::new("file_b.txt"));
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_resolve_data_dir_with_env_var() {
        // Save and restore env var
        let original = std::env::var("RUST_PAD_DATA_DIR").ok();
        std::env::set_var("RUST_PAD_DATA_DIR", "/custom/path");
        let dir = resolve_data_dir();
        assert_eq!(dir, PathBuf::from("/custom/path"));
        // Restore
        match original {
            Some(val) => std::env::set_var("RUST_PAD_DATA_DIR", val),
            None => std::env::remove_var("RUST_PAD_DATA_DIR"),
        }
    }
}
