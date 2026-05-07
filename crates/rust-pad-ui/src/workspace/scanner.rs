/// Directory scanning and incremental tree updates.
use std::path::Path;

use anyhow::{Context, Result};

use super::tree::{EntryKind, FolderRoot, TreeEntry};
use super::watcher::FsEvent;

/// Maximum entries to load per directory (prevents UI slowdown on huge dirs).
const MAX_ENTRIES_PER_DIR: usize = 10_000;

/// Scans one level of a directory and returns sorted entries.
///
/// Directories are listed first, then files. Both groups are sorted
/// alphabetically (case-insensitive). Hidden files (starting with `.`)
/// are skipped.
pub fn scan_directory(path: &Path) -> Result<Vec<TreeEntry>> {
    let read_dir = std::fs::read_dir(path)
        .with_context(|| format!("Failed to read directory: {}", path.display()))?;

    let mut entries = Vec::new();
    for entry_result in read_dir {
        let entry = match entry_result {
            Ok(e) => e,
            Err(e) => {
                tracing::warn!("Skipping unreadable entry in {}: {e}", path.display());
                continue;
            }
        };

        let name = entry.file_name().to_string_lossy().into_owned();

        // Skip hidden files/folders
        if name.starts_with('.') {
            continue;
        }

        let file_type = match entry.file_type() {
            Ok(ft) => ft,
            Err(e) => {
                tracing::warn!("Could not determine file type for {name}: {e}");
                continue;
            }
        };

        let kind = if file_type.is_dir() {
            EntryKind::Directory
        } else {
            EntryKind::File
        };

        entries.push(TreeEntry {
            name,
            path: entry.path(),
            kind,
            expanded: false,
            children: Vec::new(),
        });

        if entries.len() >= MAX_ENTRIES_PER_DIR {
            tracing::warn!(
                "Directory {} has more than {MAX_ENTRIES_PER_DIR} entries, truncating",
                path.display()
            );
            break;
        }
    }

    sort_entries(&mut entries);
    Ok(entries)
}

/// Sorts entries: directories first, then files. Both alphabetically
/// (case-insensitive).
fn sort_entries(entries: &mut [TreeEntry]) {
    entries.sort_by(|a, b| {
        // Directories before files
        let kind_ord = match (a.kind, b.kind) {
            (EntryKind::Directory, EntryKind::File) => std::cmp::Ordering::Less,
            (EntryKind::File, EntryKind::Directory) => std::cmp::Ordering::Greater,
            _ => std::cmp::Ordering::Equal,
        };
        kind_ord.then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
}

/// Applies a filesystem event to the tree, updating it incrementally.
pub fn apply_fs_event(roots: &mut [FolderRoot], event: &FsEvent) {
    match event {
        FsEvent::Created(path) | FsEvent::Modified(path) => {
            // For Modified events on existing entries, no tree change needed.
            // For Created events (or Modified on paths not in tree), insert.
            if let Some(parent_entries) = find_parent_entries(roots, path) {
                // Check if already present
                let already_exists = parent_entries.iter().any(|e| e.path == *path);
                if already_exists {
                    return;
                }

                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();

                // Skip hidden
                if name.starts_with('.') {
                    return;
                }

                let kind = if path.is_dir() {
                    EntryKind::Directory
                } else {
                    EntryKind::File
                };

                let new_entry = TreeEntry {
                    name,
                    path: path.clone(),
                    kind,
                    expanded: false,
                    children: Vec::new(),
                };

                parent_entries.push(new_entry);
                sort_entries(parent_entries);
            }
        }
        FsEvent::Removed(path) => {
            if let Some(parent_entries) = find_parent_entries(roots, path) {
                parent_entries.retain(|e| e.path != *path);
            }
        }
    }
}

/// Finds the parent directory's children list for a given path.
///
/// Walks the tree to locate the `Vec<TreeEntry>` that should contain
/// the entry at `path`.
pub fn find_parent_entries<'a>(
    roots: &'a mut [FolderRoot],
    path: &Path,
) -> Option<&'a mut Vec<TreeEntry>> {
    let parent = path.parent()?;

    for root in roots.iter_mut() {
        if parent == root.path {
            return Some(&mut root.entries);
        }

        if let Some(entries) = find_in_children(&mut root.entries, parent) {
            return Some(entries);
        }
    }

    None
}

/// Recursively searches children for a directory matching `target_parent`.
fn find_in_children<'a>(
    entries: &'a mut [TreeEntry],
    target_parent: &Path,
) -> Option<&'a mut Vec<TreeEntry>> {
    for entry in entries.iter_mut() {
        if entry.kind == EntryKind::Directory {
            if entry.path == target_parent {
                return Some(&mut entry.children);
            }
            if target_parent.starts_with(&entry.path) {
                if let Some(found) = find_in_children(&mut entry.children, target_parent) {
                    return Some(found);
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tempfile::TempDir;

    #[test]
    fn test_scan_directory_basic() {
        let dir = TempDir::new().expect("create temp dir");

        // Create some files and directories
        std::fs::create_dir(dir.path().join("alpha_dir")).expect("mkdir");
        std::fs::create_dir(dir.path().join("beta_dir")).expect("mkdir");
        std::fs::write(dir.path().join("charlie.txt"), "").expect("write");
        std::fs::write(dir.path().join("able.rs"), "").expect("write");

        let entries = scan_directory(dir.path()).expect("scan");

        // Directories first, then files
        assert_eq!(entries.len(), 4);
        assert_eq!(entries[0].kind, EntryKind::Directory);
        assert_eq!(entries[0].name, "alpha_dir");
        assert_eq!(entries[1].kind, EntryKind::Directory);
        assert_eq!(entries[1].name, "beta_dir");
        assert_eq!(entries[2].kind, EntryKind::File);
        assert_eq!(entries[2].name, "able.rs");
        assert_eq!(entries[3].kind, EntryKind::File);
        assert_eq!(entries[3].name, "charlie.txt");
    }

    #[test]
    fn test_scan_directory_skips_hidden() {
        let dir = TempDir::new().expect("create temp dir");

        std::fs::write(dir.path().join(".hidden"), "").expect("write");
        std::fs::create_dir(dir.path().join(".git")).expect("mkdir");
        std::fs::write(dir.path().join("visible.txt"), "").expect("write");

        let entries = scan_directory(dir.path()).expect("scan");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "visible.txt");
    }

    #[test]
    fn test_scan_directory_sort_case_insensitive() {
        let dir = TempDir::new().expect("create temp dir");

        std::fs::write(dir.path().join("Zebra.txt"), "").expect("write");
        std::fs::write(dir.path().join("apple.txt"), "").expect("write");
        std::fs::write(dir.path().join("Banana.txt"), "").expect("write");

        let entries = scan_directory(dir.path()).expect("scan");
        assert_eq!(entries[0].name, "apple.txt");
        assert_eq!(entries[1].name, "Banana.txt");
        assert_eq!(entries[2].name, "Zebra.txt");
    }

    #[test]
    fn test_apply_fs_event_created() {
        let dir = TempDir::new().expect("create temp dir");
        let existing = dir.path().join("existing.rs");
        std::fs::write(&existing, "").expect("write");

        let mut roots = vec![FolderRoot {
            path: dir.path().to_path_buf(),
            entries: vec![TreeEntry {
                name: "existing.rs".to_string(),
                path: existing,
                kind: EntryKind::File,
                expanded: false,
                children: Vec::new(),
            }],
            expanded: true,
        }];

        // Create a real file and fire a Created event
        let new_path = dir.path().join("new_file.txt");
        std::fs::write(&new_path, "hello").expect("write");
        apply_fs_event(&mut roots, &FsEvent::Created(new_path.clone()));

        assert_eq!(roots[0].entries.len(), 2);
        // Entries should be sorted: existing.rs, new_file.txt
        assert_eq!(roots[0].entries[1].name, "new_file.txt");
        assert_eq!(roots[0].entries[1].kind, EntryKind::File);
    }

    #[test]
    fn test_apply_fs_event_removed() {
        let mut roots = vec![FolderRoot {
            path: PathBuf::from("/project"),
            entries: vec![
                TreeEntry {
                    name: "keep.rs".to_string(),
                    path: PathBuf::from("/project/keep.rs"),
                    kind: EntryKind::File,
                    expanded: false,
                    children: Vec::new(),
                },
                TreeEntry {
                    name: "remove.rs".to_string(),
                    path: PathBuf::from("/project/remove.rs"),
                    kind: EntryKind::File,
                    expanded: false,
                    children: Vec::new(),
                },
            ],
            expanded: true,
        }];

        apply_fs_event(
            &mut roots,
            &FsEvent::Removed(PathBuf::from("/project/remove.rs")),
        );

        assert_eq!(roots[0].entries.len(), 1);
        assert_eq!(roots[0].entries[0].name, "keep.rs");
    }

    #[test]
    fn test_find_parent_entries_root_level() {
        let mut roots = vec![FolderRoot {
            path: PathBuf::from("/project"),
            entries: Vec::new(),
            expanded: true,
        }];

        let result = find_parent_entries(&mut roots, &PathBuf::from("/project/file.txt"));
        assert!(result.is_some());
    }

    #[test]
    fn test_find_parent_entries_nested() {
        let mut roots = vec![FolderRoot {
            path: PathBuf::from("/project"),
            entries: vec![TreeEntry {
                name: "src".to_string(),
                path: PathBuf::from("/project/src"),
                kind: EntryKind::Directory,
                expanded: true,
                children: Vec::new(),
            }],
            expanded: true,
        }];

        let result = find_parent_entries(&mut roots, &PathBuf::from("/project/src/main.rs"));
        assert!(result.is_some());
    }

    #[test]
    fn test_find_parent_entries_not_found() {
        let mut roots = vec![FolderRoot {
            path: PathBuf::from("/project"),
            entries: Vec::new(),
            expanded: true,
        }];

        let result = find_parent_entries(&mut roots, &PathBuf::from("/other/file.txt"));
        assert!(result.is_none());
    }

    #[test]
    fn test_scan_nonexistent_directory_returns_error() {
        let result = scan_directory(Path::new("/nonexistent_dir_xyz_123"));
        assert!(result.is_err());
    }

    #[test]
    fn test_scan_empty_directory() {
        let dir = TempDir::new().expect("create temp dir");
        let entries = scan_directory(dir.path()).expect("scan");
        assert!(entries.is_empty());
    }

    #[test]
    fn test_apply_fs_event_modified_existing_is_noop() {
        let mut roots = vec![FolderRoot {
            path: PathBuf::from("/project"),
            entries: vec![TreeEntry {
                name: "file.rs".to_string(),
                path: PathBuf::from("/project/file.rs"),
                kind: EntryKind::File,
                expanded: false,
                children: Vec::new(),
            }],
            expanded: true,
        }];

        // Modified event for existing entry should not duplicate it
        apply_fs_event(
            &mut roots,
            &FsEvent::Modified(PathBuf::from("/project/file.rs")),
        );
        assert_eq!(roots[0].entries.len(), 1);
    }

    #[test]
    fn test_apply_fs_event_created_hidden_file_ignored() {
        let dir = TempDir::new().expect("create temp dir");
        let hidden = dir.path().join(".hidden_file");
        std::fs::write(&hidden, "").expect("write");

        let mut roots = vec![FolderRoot {
            path: dir.path().to_path_buf(),
            entries: Vec::new(),
            expanded: true,
        }];

        apply_fs_event(&mut roots, &FsEvent::Created(hidden));
        assert!(
            roots[0].entries.is_empty(),
            "Hidden files should not be added to the tree"
        );
    }

    #[test]
    fn test_apply_fs_event_created_duplicate_ignored() {
        let mut roots = vec![FolderRoot {
            path: PathBuf::from("/project"),
            entries: vec![TreeEntry {
                name: "file.rs".to_string(),
                path: PathBuf::from("/project/file.rs"),
                kind: EntryKind::File,
                expanded: false,
                children: Vec::new(),
            }],
            expanded: true,
        }];

        apply_fs_event(
            &mut roots,
            &FsEvent::Created(PathBuf::from("/project/file.rs")),
        );
        assert_eq!(
            roots[0].entries.len(),
            1,
            "Duplicate Created event should not add a second entry"
        );
    }

    #[test]
    fn test_apply_fs_event_removed_nonexistent_is_noop() {
        let mut roots = vec![FolderRoot {
            path: PathBuf::from("/project"),
            entries: vec![TreeEntry {
                name: "keep.rs".to_string(),
                path: PathBuf::from("/project/keep.rs"),
                kind: EntryKind::File,
                expanded: false,
                children: Vec::new(),
            }],
            expanded: true,
        }];

        apply_fs_event(
            &mut roots,
            &FsEvent::Removed(PathBuf::from("/project/gone.rs")),
        );
        assert_eq!(roots[0].entries.len(), 1);
        assert_eq!(roots[0].entries[0].name, "keep.rs");
    }

    #[test]
    fn test_find_parent_entries_deeply_nested() {
        let mut roots = vec![FolderRoot {
            path: PathBuf::from("/project"),
            entries: vec![TreeEntry {
                name: "src".to_string(),
                path: PathBuf::from("/project/src"),
                kind: EntryKind::Directory,
                expanded: true,
                children: vec![TreeEntry {
                    name: "utils".to_string(),
                    path: PathBuf::from("/project/src/utils"),
                    kind: EntryKind::Directory,
                    expanded: true,
                    children: Vec::new(),
                }],
            }],
            expanded: true,
        }];

        let result =
            find_parent_entries(&mut roots, &PathBuf::from("/project/src/utils/helper.rs"));
        assert!(result.is_some(), "Should find parent at depth 3");
    }
}
