/// Platform-standard path resolution and legacy-path migration.
///
/// On first launch after upgrading, files are automatically copied from
/// the old exe-relative locations to the platform-standard directories.
/// Originals are preserved so that older versions of the app still work.
use std::path::{Path, PathBuf};

use crate::permissions::{set_owner_only_dir_permissions, set_owner_only_file_permissions};

/// Sub-directory name under the platform config/data root.
const APP_DIR_NAME: &str = "rust-pad";

const CONFIG_FILE_NAME: &str = "rust-pad.json";
const SESSION_FILE_NAME: &str = "rust-pad-session.redb";
const HISTORY_FILE_NAME: &str = "history.redb";

/// Legacy sub-directory name for history data (next to the executable).
const LEGACY_HISTORY_DIR: &str = ".data";

// ── Public path accessors ──────────────────────────────────────────

/// Returns the directory where configuration files should live.
///
/// Resolution:
/// 1. `dirs::config_dir() / rust-pad/` (platform standard)
/// 2. Executable directory (fallback)
pub fn app_config_dir() -> PathBuf {
    dirs::config_dir()
        .map(|d| d.join(APP_DIR_NAME))
        .unwrap_or_else(|| {
            tracing::warn!(
                "Could not determine platform config directory; using executable directory"
            );
            exe_dir()
        })
}

/// Returns the directory where data files (databases) should live.
///
/// Resolution:
/// 1. `dirs::data_dir() / rust-pad/` (platform standard)
/// 2. Executable directory (fallback)
pub fn app_data_dir() -> PathBuf {
    dirs::data_dir()
        .map(|d| d.join(APP_DIR_NAME))
        .unwrap_or_else(|| {
            tracing::warn!(
                "Could not determine platform data directory; using executable directory"
            );
            exe_dir()
        })
}

/// Returns the full path for the config JSON file.
pub fn config_file_path() -> PathBuf {
    app_config_dir().join(CONFIG_FILE_NAME)
}

/// Returns the full path for the session database.
pub fn session_file_path() -> PathBuf {
    app_data_dir().join(SESSION_FILE_NAME)
}

/// Returns the directory where the history database lives.
///
/// Resolution:
/// 1. `RUST_PAD_DATA_DIR` environment variable (if set)
/// 2. `dirs::data_dir() / rust-pad/` (platform standard)
/// 3. Executable directory (fallback)
pub fn history_data_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("RUST_PAD_DATA_DIR") {
        return PathBuf::from(dir);
    }
    app_data_dir()
}

// ── Migration ──────────────────────────────────────────────────────

/// Migrates config and data files from legacy exe-relative paths to
/// platform-standard directories.
///
/// Safe to call multiple times — files are only copied if the
/// destination does not already exist. Originals are preserved so
/// that older versions of the application continue to work.
///
/// Failures are logged as warnings but never prevent the app from
/// starting.
pub fn migrate_legacy_paths() {
    let exe = exe_dir();

    // Config: {exe}/rust-pad.json → {config_dir}/rust-pad/rust-pad.json
    let new_config = config_file_path();
    let old_config = exe.join(CONFIG_FILE_NAME);
    migrate_file(&old_config, &new_config, "config");

    // Session: {exe}/rust-pad-session.redb → {data_dir}/rust-pad/rust-pad-session.redb
    let new_session = session_file_path();
    let old_session = exe.join(SESSION_FILE_NAME);
    migrate_file(&old_session, &new_session, "session database");

    // History: {exe}/.data/history.redb → {data_dir}/rust-pad/history.redb
    let new_history_dir = history_data_dir();
    let new_history = new_history_dir.join(HISTORY_FILE_NAME);
    let old_history = exe.join(LEGACY_HISTORY_DIR).join(HISTORY_FILE_NAME);
    migrate_file(&old_history, &new_history, "history database");
}

// ── Portable (exe-relative) path accessors ─────────────────────────

/// Returns the config file path next to the executable (portable mode).
pub fn portable_config_file_path() -> PathBuf {
    exe_dir().join(CONFIG_FILE_NAME)
}

/// Returns the session file path next to the executable (portable mode).
pub fn portable_session_file_path() -> PathBuf {
    exe_dir().join(SESSION_FILE_NAME)
}

/// Returns the history data directory next to the executable (portable mode).
///
/// Respects `RUST_PAD_DATA_DIR` if set.
pub fn portable_history_data_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("RUST_PAD_DATA_DIR") {
        return PathBuf::from(dir);
    }
    exe_dir().join(LEGACY_HISTORY_DIR)
}

// ── Internal helpers ───────────────────────────────────────────────

/// Returns the parent directory of the running executable.
fn exe_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Copies `old` to `new` if `old` exists and `new` does not.
///
/// Creates parent directories as needed and sets restrictive
/// permissions on both the directory and the copied file.
fn migrate_file(old: &Path, new: &Path, label: &str) {
    // Skip if the old and new paths resolve to the same location
    // (happens when dirs falls back to exe dir).
    if paths_equivalent(old, new) {
        return;
    }

    // Already at the new location — nothing to do.
    if new.exists() {
        return;
    }

    // Nothing to migrate.
    if !old.exists() {
        return;
    }

    // Ensure the target directory exists.
    if let Some(parent) = new.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            tracing::warn!(
                "Failed to create directory '{}' for {label} migration: {e}",
                parent.display()
            );
            return;
        }
        set_owner_only_dir_permissions(parent);
    }

    // Copy the file (preserve the original for downgrade safety).
    match std::fs::copy(old, new) {
        Ok(_) => {
            set_owner_only_file_permissions(new);
            tracing::info!(
                "Migrated {label}: '{}' → '{}'",
                old.display(),
                new.display()
            );
        }
        Err(e) => {
            tracing::warn!(
                "Failed to migrate {label} from '{}' to '{}': {e}",
                old.display(),
                new.display()
            );
        }
    }
}

/// Returns `true` if two paths point to the same location.
///
/// Compares canonical paths when possible, falls back to string comparison.
fn paths_equivalent(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(ca), Ok(cb)) => ca == cb,
        _ => a == b,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_app_config_dir_returns_non_empty_path() {
        let dir = app_config_dir();
        assert!(!dir.as_os_str().is_empty());
        assert!(dir.ends_with(APP_DIR_NAME) || dir == exe_dir());
    }

    #[test]
    fn test_app_data_dir_returns_non_empty_path() {
        let dir = app_data_dir();
        assert!(!dir.as_os_str().is_empty());
        assert!(dir.ends_with(APP_DIR_NAME) || dir == exe_dir());
    }

    #[test]
    fn test_config_file_path_ends_with_expected_name() {
        let path = config_file_path();
        assert!(path.ends_with(CONFIG_FILE_NAME));
    }

    #[test]
    fn test_session_file_path_ends_with_expected_name() {
        let path = session_file_path();
        assert!(path.ends_with(SESSION_FILE_NAME));
    }

    #[test]
    fn test_history_data_dir_respects_env_var() {
        let original = std::env::var("RUST_PAD_DATA_DIR").ok();
        std::env::set_var("RUST_PAD_DATA_DIR", "/custom/data");
        let dir = history_data_dir();
        assert_eq!(dir, PathBuf::from("/custom/data"));
        match original {
            Some(val) => std::env::set_var("RUST_PAD_DATA_DIR", val),
            None => std::env::remove_var("RUST_PAD_DATA_DIR"),
        }
    }

    #[test]
    fn test_migrate_file_copies_when_old_exists() {
        let dir = TempDir::new().expect("temp dir");
        let old_dir = dir.path().join("old");
        let new_dir = dir.path().join("new");
        std::fs::create_dir_all(&old_dir).expect("mkdir old");

        let old_file = old_dir.join("test.txt");
        let new_file = new_dir.join("test.txt");
        std::fs::write(&old_file, "hello").expect("write");

        migrate_file(&old_file, &new_file, "test");

        assert!(new_file.exists(), "new file should exist after migration");
        assert!(old_file.exists(), "old file should be preserved");
        assert_eq!(std::fs::read_to_string(&new_file).expect("read"), "hello");
    }

    #[test]
    fn test_migrate_file_skips_when_new_exists() {
        let dir = TempDir::new().expect("temp dir");
        let old_file = dir.path().join("old.txt");
        let new_file = dir.path().join("new.txt");
        std::fs::write(&old_file, "old content").expect("write old");
        std::fs::write(&new_file, "new content").expect("write new");

        migrate_file(&old_file, &new_file, "test");

        // New file should keep its content (not overwritten).
        assert_eq!(
            std::fs::read_to_string(&new_file).expect("read"),
            "new content"
        );
    }

    #[test]
    fn test_migrate_file_noop_when_old_missing() {
        let dir = TempDir::new().expect("temp dir");
        let old_file = dir.path().join("nonexistent.txt");
        let new_file = dir.path().join("new.txt");

        migrate_file(&old_file, &new_file, "test");

        assert!(!new_file.exists());
    }

    #[test]
    fn test_migrate_file_skips_when_paths_equivalent() {
        let dir = TempDir::new().expect("temp dir");
        let file = dir.path().join("same.txt");
        std::fs::write(&file, "data").expect("write");

        // Migrating a file to itself should be a no-op.
        migrate_file(&file, &file, "test");

        assert_eq!(std::fs::read_to_string(&file).expect("read"), "data");
    }

    #[test]
    fn test_paths_equivalent_same_path() {
        let dir = TempDir::new().expect("temp dir");
        let file = dir.path().join("test.txt");
        std::fs::write(&file, "").expect("write");
        assert!(paths_equivalent(&file, &file));
    }

    #[test]
    fn test_paths_equivalent_different_paths() {
        let dir = TempDir::new().expect("temp dir");
        let a = dir.path().join("a.txt");
        let b = dir.path().join("b.txt");
        std::fs::write(&a, "").expect("write a");
        std::fs::write(&b, "").expect("write b");
        assert!(!paths_equivalent(&a, &b));
    }
}
