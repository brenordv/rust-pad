/// Cross-platform file and directory permission helpers.
///
/// On Unix, restricts access to the owning user only (0700 for directories,
/// 0600 for files). On Windows, this is a no-op — NTFS user profile
/// directories already have user-scoped ACLs by default.
///
/// All functions log a warning on failure but never return an error,
/// so the application can continue even on filesystems that don't
/// support permission changes (e.g. FAT32, network mounts).
use std::path::Path;

/// Sets directory permissions to owner-only (0700 on Unix).
///
/// Logs a warning if the operation fails. Does nothing on Windows.
pub fn set_owner_only_dir_permissions(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o700);
        if let Err(e) = std::fs::set_permissions(path, perms) {
            tracing::warn!(
                "Failed to set directory permissions (0700) on '{}': {e}",
                path.display()
            );
        }
    }

    #[cfg(not(unix))]
    {
        let _ = path;
    }
}

/// Sets file permissions to owner-only read/write (0600 on Unix).
///
/// Logs a warning if the operation fails. Does nothing on Windows.
pub fn set_owner_only_file_permissions(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o600);
        if let Err(e) = std::fs::set_permissions(path, perms) {
            tracing::warn!(
                "Failed to set file permissions (0600) on '{}': {e}",
                path.display()
            );
        }
    }

    #[cfg(not(unix))]
    {
        let _ = path;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Verifying the functions don't panic on a valid path.
    #[test]
    fn test_set_dir_permissions_on_valid_dir() {
        let dir = tempfile::tempdir().expect("create temp dir");
        set_owner_only_dir_permissions(dir.path());
        // Should not panic; on Unix, verify the mode
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let meta = std::fs::metadata(dir.path()).expect("metadata");
            assert_eq!(meta.permissions().mode() & 0o777, 0o700);
        }
    }

    #[test]
    fn test_set_file_permissions_on_valid_file() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let file_path = dir.path().join("test.txt");
        std::fs::write(&file_path, "hello").expect("write");
        set_owner_only_file_permissions(&file_path);
        // Should not panic; on Unix, verify the mode
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let meta = std::fs::metadata(&file_path).expect("metadata");
            assert_eq!(meta.permissions().mode() & 0o777, 0o600);
        }
    }

    /// Non-existent path should warn but not panic.
    #[test]
    fn test_set_permissions_on_nonexistent_path_does_not_panic() {
        let bogus = Path::new("/tmp/rust-pad-nonexistent-path-for-test-12345");
        set_owner_only_dir_permissions(bogus);
        set_owner_only_file_permissions(bogus);
    }
}
