/// Directory scanning and incremental tree updates.
use std::collections::HashSet;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use super::tree::{EntryKind, FolderRoot, TreeEntry};
use super::watcher::FsEvent;

/// Maximum entries to load per directory (prevents UI slowdown on huge dirs).
const MAX_ENTRIES_PER_DIR: usize = 10_000;

/// Checks whether a filesystem entry should be considered hidden.
///
/// On all platforms, names starting with `.` are considered hidden.
/// On Windows, the `FILE_ATTRIBUTE_HIDDEN` file attribute is also checked.
fn is_hidden(name: &str, path: &Path) -> bool {
    if name.starts_with('.') {
        return true;
    }
    is_os_hidden(path)
}

#[cfg(windows)]
fn is_os_hidden(path: &Path) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;
    path.metadata()
        .map(|m| m.file_attributes() & FILE_ATTRIBUTE_HIDDEN != 0)
        .unwrap_or(false)
}

#[cfg(not(windows))]
fn is_os_hidden(_path: &Path) -> bool {
    false
}

/// Scans one level of a directory and returns sorted entries.
///
/// Directories are listed first, then files. Both groups are sorted
/// alphabetically (case-insensitive). Hidden files (starting with `.`)
/// are skipped unless `show_hidden` is true.
pub fn scan_directory(path: &Path, show_hidden: bool) -> Result<Vec<TreeEntry>> {
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

        // Skip hidden files/folders unless show_hidden is enabled
        if !show_hidden && is_hidden(&name, &entry.path()) {
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

/// Outcome of reconciling a directory against disk.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ReconcileSummary {
    /// Entries newly discovered on disk and inserted into the tree.
    pub added: usize,
    /// Entries gone from disk and removed from the tree.
    pub removed: usize,
}

/// Re-reads `dir_path` one level deep and reconciles the fresh listing into
/// `existing`.
///
/// Entries that still exist keep their `expanded` flag and already-loaded
/// `children`; newcomers are inserted; entries no longer on disk are dropped.
/// The result is re-sorted into the canonical order.
///
/// # Errors
/// Returns the underlying read error when `dir_path` cannot be scanned; in
/// that case `existing` is left unchanged.
pub fn reconcile_directory(
    dir_path: &Path,
    existing: &mut Vec<TreeEntry>,
    show_hidden: bool,
) -> Result<ReconcileSummary> {
    let fresh = scan_directory(dir_path, show_hidden)?;

    // Drop entries that vanished from disk.
    let fresh_paths: HashSet<PathBuf> = fresh.iter().map(|e| e.path.clone()).collect();
    let before = existing.len();
    existing.retain(|e| fresh_paths.contains(&e.path));
    let removed = before - existing.len();

    // Insert newcomers, preserving the expansion state of survivors.
    let existing_paths: HashSet<PathBuf> = existing.iter().map(|e| e.path.clone()).collect();
    let mut added = 0;
    for entry in fresh {
        if !existing_paths.contains(&entry.path) {
            existing.push(entry);
            added += 1;
        }
    }

    sort_entries(existing);
    Ok(ReconcileSummary { added, removed })
}

/// Reconciles `entries` against `dir_path`, then recurses into every child
/// that is still an expanded directory, refreshing the whole currently
/// *visible* subtree.
///
/// This is what lets a reload pick up folders created deep inside an
/// already-expanded tree — the case the one-level [`reconcile_directory`]
/// alone would miss. Symlinks are classified as files by [`scan_directory`],
/// so recursion never follows them (no cycles, no tree escape). A read
/// failure on one nested directory is logged and skipped so it cannot abort
/// the rest of the refresh.
pub fn reconcile_tree_recursive(
    dir_path: &Path,
    entries: &mut Vec<TreeEntry>,
    show_hidden: bool,
) -> ReconcileSummary {
    let mut summary = match reconcile_directory(dir_path, entries, show_hidden) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!("Failed to reload directory {}: {e}", dir_path.display());
            return ReconcileSummary::default();
        }
    };

    for entry in entries.iter_mut() {
        if entry.kind == EntryKind::Directory && entry.expanded {
            let child_path = entry.path.clone();
            let child = reconcile_tree_recursive(&child_path, &mut entry.children, show_hidden);
            summary.added += child.added;
            summary.removed += child.removed;
        }
    }

    summary
}

/// Applies a filesystem event to the tree, updating it incrementally.
///
/// `Modified`/`Created` events whose path is itself an already-loaded
/// directory are reconciled from disk rather than treated as a no-op. This
/// is essential on macOS, where the `notify` FSEvents backend coalesces a
/// burst of changes into a single event for the *containing directory*: a
/// plain insert would find the directory already present and drop the newly
/// created children. Leaf events (Windows `ReadDirectoryChangesW`, Linux
/// `inotify`) fall through to the fast-path insert.
pub fn apply_fs_event(roots: &mut [FolderRoot], event: &FsEvent, show_hidden: bool) {
    match event {
        FsEvent::Created(path) | FsEvent::Modified(path) => {
            if !reconcile_if_loaded_dir(roots, path, show_hidden) {
                insert_entry_if_new(roots, path, show_hidden);
            }
        }
        FsEvent::Removed(path) => {
            if let Some(parent_entries) = find_parent_entries(roots, path) {
                parent_entries.retain(|e| e.path != *path);
            }
        }
    }
}

/// If `path` is a directory already represented in the tree, reconcile it (and
/// its expanded subtree) from disk and return `true`. Returns `false` when
/// `path` is not a known directory node, so the caller can fall back to a
/// leaf insert.
fn reconcile_if_loaded_dir(roots: &mut [FolderRoot], path: &Path, show_hidden: bool) -> bool {
    if !path.is_dir() {
        return false;
    }

    for root in roots.iter_mut() {
        if root.path == path {
            let summary = reconcile_tree_recursive(path, &mut root.entries, show_hidden);
            tracing::debug!(
                path = %path.display(),
                added = summary.added,
                removed = summary.removed,
                "fs event reconciled workspace root"
            );
            return true;
        }
        if let Some(entry) = find_entry_mut(&mut root.entries, path) {
            if entry.kind == EntryKind::Directory {
                let dir_path = entry.path.clone();
                let summary = reconcile_tree_recursive(&dir_path, &mut entry.children, show_hidden);
                tracing::debug!(
                    path = %path.display(),
                    added = summary.added,
                    removed = summary.removed,
                    "fs event reconciled directory"
                );
                return true;
            }
        }
    }

    false
}

/// Inserts a new tree entry for `path` if it doesn't already exist.
///
/// Used for both Created and Modified events — Modified events on existing
/// entries are no-ops, while new paths are inserted and sorted.
fn insert_entry_if_new(roots: &mut [FolderRoot], path: &Path, show_hidden: bool) {
    let Some(parent_entries) = find_parent_entries(roots, path) else {
        // No parent node in the tree. On macOS this is the tell-tale of a
        // canonicalised (symlink-resolved) event path that no longer matches
        // the stored root path — see the FSEvents notes on `apply_fs_event`.
        tracing::debug!(path = %path.display(), "fs event dropped: parent not in tree");
        return;
    };

    if parent_entries.iter().any(|e| e.path == *path) {
        return;
    }

    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();

    // Skip hidden files unless show_hidden is enabled
    if !show_hidden && is_hidden(&name, path) {
        return;
    }

    let kind = if path.is_dir() {
        EntryKind::Directory
    } else {
        EntryKind::File
    };

    parent_entries.push(TreeEntry {
        name,
        path: path.to_path_buf(),
        kind,
        expanded: false,
        children: Vec::new(),
    });
    sort_entries(parent_entries);
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

/// Walks `entries` recursively looking for the node at `target`, yielding a
/// mutable reference so callers can reconcile that directory's own
/// `children` in place. The mutable counterpart of `find_entry` in
/// `sidebar.rs`; kept here so the reload handler reuses one locator instead
/// of hand-rolling a recursive mutable walk.
pub fn find_entry_mut<'a>(
    entries: &'a mut [TreeEntry],
    target: &Path,
) -> Option<&'a mut TreeEntry> {
    for entry in entries.iter_mut() {
        if entry.path == target {
            return Some(entry);
        }
        if entry.kind == EntryKind::Directory {
            if let Some(hit) = find_entry_mut(&mut entry.children, target) {
                return Some(hit);
            }
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

        let entries = scan_directory(dir.path(), false).expect("scan");

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

        let entries = scan_directory(dir.path(), false).expect("scan");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "visible.txt");
    }

    #[test]
    fn test_scan_directory_shows_hidden_when_enabled() {
        let dir = TempDir::new().expect("create temp dir");

        std::fs::write(dir.path().join(".hidden"), "").expect("write");
        std::fs::create_dir(dir.path().join(".git")).expect("mkdir");
        std::fs::write(dir.path().join("visible.txt"), "").expect("write");

        let entries = scan_directory(dir.path(), true).expect("scan");
        assert_eq!(entries.len(), 3);
        // Directories first, then files (alphabetical within each group)
        assert_eq!(entries[0].name, ".git");
        assert_eq!(entries[0].kind, EntryKind::Directory);
        assert_eq!(entries[1].name, ".hidden");
        assert_eq!(entries[1].kind, EntryKind::File);
        assert_eq!(entries[2].name, "visible.txt");
        assert_eq!(entries[2].kind, EntryKind::File);
    }

    #[test]
    fn test_scan_directory_sort_case_insensitive() {
        let dir = TempDir::new().expect("create temp dir");

        std::fs::write(dir.path().join("Zebra.txt"), "").expect("write");
        std::fs::write(dir.path().join("apple.txt"), "").expect("write");
        std::fs::write(dir.path().join("Banana.txt"), "").expect("write");

        let entries = scan_directory(dir.path(), false).expect("scan");
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
        apply_fs_event(&mut roots, &FsEvent::Created(new_path.clone()), false);

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
            false,
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
        let result = scan_directory(Path::new("/nonexistent_dir_xyz_123"), false);
        assert!(result.is_err());
    }

    #[test]
    fn test_scan_empty_directory() {
        let dir = TempDir::new().expect("create temp dir");
        let entries = scan_directory(dir.path(), false).expect("scan");
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
            false,
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

        apply_fs_event(&mut roots, &FsEvent::Created(hidden), false);
        assert!(
            roots[0].entries.is_empty(),
            "Hidden files should not be added to the tree"
        );
    }

    #[test]
    fn test_apply_fs_event_created_hidden_file_included_when_show_hidden() {
        let dir = TempDir::new().expect("create temp dir");
        let hidden = dir.path().join(".hidden_file");
        std::fs::write(&hidden, "").expect("write");

        let mut roots = vec![FolderRoot {
            path: dir.path().to_path_buf(),
            entries: Vec::new(),
            expanded: true,
        }];

        apply_fs_event(&mut roots, &FsEvent::Created(hidden), true);
        assert_eq!(roots[0].entries.len(), 1);
        assert_eq!(roots[0].entries[0].name, ".hidden_file");
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
            false,
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
            false,
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

    #[test]
    fn test_apply_fs_event_created_directory() {
        let dir = TempDir::new().expect("create temp dir");
        let new_dir = dir.path().join("new_subdir");
        std::fs::create_dir(&new_dir).expect("mkdir");

        let mut roots = vec![FolderRoot {
            path: dir.path().to_path_buf(),
            entries: Vec::new(),
            expanded: true,
        }];

        apply_fs_event(&mut roots, &FsEvent::Created(new_dir), false);

        assert_eq!(roots[0].entries.len(), 1);
        assert_eq!(roots[0].entries[0].name, "new_subdir");
        assert_eq!(roots[0].entries[0].kind, EntryKind::Directory);
    }

    #[test]
    fn test_apply_fs_event_created_no_parent_in_tree() {
        let mut roots = vec![FolderRoot {
            path: PathBuf::from("/project"),
            entries: Vec::new(),
            expanded: true,
        }];

        // Event for a file under /other (not in the tree) — should be a no-op
        apply_fs_event(
            &mut roots,
            &FsEvent::Created(PathBuf::from("/other/file.rs")),
            false,
        );

        assert!(
            roots[0].entries.is_empty(),
            "Should not insert when parent is not in tree"
        );
    }

    #[test]
    fn test_apply_fs_event_in_nested_directory() {
        let dir = TempDir::new().expect("create temp dir");
        let src_dir = dir.path().join("src");
        std::fs::create_dir(&src_dir).expect("mkdir src");
        let new_file = src_dir.join("lib.rs");
        std::fs::write(&new_file, "").expect("write");

        let mut roots = vec![FolderRoot {
            path: dir.path().to_path_buf(),
            entries: vec![TreeEntry {
                name: "src".to_string(),
                path: src_dir,
                kind: EntryKind::Directory,
                expanded: true,
                children: Vec::new(),
            }],
            expanded: true,
        }];

        apply_fs_event(&mut roots, &FsEvent::Created(new_file.clone()), false);

        assert_eq!(roots[0].entries[0].children.len(), 1);
        assert_eq!(roots[0].entries[0].children[0].name, "lib.rs");
    }

    #[test]
    fn test_apply_fs_event_removed_from_nested() {
        let mut roots = vec![FolderRoot {
            path: PathBuf::from("/project"),
            entries: vec![TreeEntry {
                name: "src".to_string(),
                path: PathBuf::from("/project/src"),
                kind: EntryKind::Directory,
                expanded: true,
                children: vec![TreeEntry {
                    name: "main.rs".to_string(),
                    path: PathBuf::from("/project/src/main.rs"),
                    kind: EntryKind::File,
                    expanded: false,
                    children: Vec::new(),
                }],
            }],
            expanded: true,
        }];

        apply_fs_event(
            &mut roots,
            &FsEvent::Removed(PathBuf::from("/project/src/main.rs")),
            false,
        );

        assert!(
            roots[0].entries[0].children.is_empty(),
            "Removed file should be deleted from nested children"
        );
    }

    #[test]
    fn test_sort_entries_dirs_before_files() {
        let mut entries = vec![
            TreeEntry {
                name: "zebra.txt".to_string(),
                path: PathBuf::from("/zebra.txt"),
                kind: EntryKind::File,
                expanded: false,
                children: Vec::new(),
            },
            TreeEntry {
                name: "alpha_dir".to_string(),
                path: PathBuf::from("/alpha_dir"),
                kind: EntryKind::Directory,
                expanded: false,
                children: Vec::new(),
            },
            TreeEntry {
                name: "beta.rs".to_string(),
                path: PathBuf::from("/beta.rs"),
                kind: EntryKind::File,
                expanded: false,
                children: Vec::new(),
            },
        ];

        sort_entries(&mut entries);

        assert_eq!(entries[0].name, "alpha_dir");
        assert_eq!(entries[0].kind, EntryKind::Directory);
        assert_eq!(entries[1].name, "beta.rs");
        assert_eq!(entries[2].name, "zebra.txt");
    }

    #[test]
    fn test_insert_preserves_sort_order() {
        let dir = TempDir::new().expect("create temp dir");
        let file_a = dir.path().join("aaa.txt");
        let file_z = dir.path().join("zzz.txt");
        let file_m = dir.path().join("mmm.txt");
        std::fs::write(&file_a, "").expect("write");
        std::fs::write(&file_z, "").expect("write");
        std::fs::write(&file_m, "").expect("write");

        let mut roots = vec![FolderRoot {
            path: dir.path().to_path_buf(),
            entries: vec![TreeEntry {
                name: "aaa.txt".to_string(),
                path: file_a,
                kind: EntryKind::File,
                expanded: false,
                children: Vec::new(),
            }],
            expanded: true,
        }];

        // Insert out of order
        apply_fs_event(&mut roots, &FsEvent::Created(file_z), false);
        apply_fs_event(&mut roots, &FsEvent::Created(file_m), false);

        assert_eq!(roots[0].entries.len(), 3);
        assert_eq!(roots[0].entries[0].name, "aaa.txt");
        assert_eq!(roots[0].entries[1].name, "mmm.txt");
        assert_eq!(roots[0].entries[2].name, "zzz.txt");
    }

    #[test]
    fn test_find_parent_entries_multiple_roots() {
        let mut roots = vec![
            FolderRoot {
                path: PathBuf::from("/project1"),
                entries: Vec::new(),
                expanded: true,
            },
            FolderRoot {
                path: PathBuf::from("/project2"),
                entries: Vec::new(),
                expanded: true,
            },
        ];

        // Should find parent in the second root
        let result = find_parent_entries(&mut roots, &PathBuf::from("/project2/file.txt"));
        assert!(result.is_some());
    }

    #[test]
    fn test_find_in_children_skips_files() {
        let mut entries = vec![TreeEntry {
            name: "not_a_dir.txt".to_string(),
            path: PathBuf::from("/project/not_a_dir.txt"),
            kind: EntryKind::File,
            expanded: false,
            children: Vec::new(),
        }];

        let result = find_in_children(&mut entries, Path::new("/project/not_a_dir.txt"));
        assert!(result.is_none(), "Should not match file entries");
    }

    #[test]
    fn test_scan_directory_dirs_first_then_files() {
        let dir = TempDir::new().expect("create temp dir");
        std::fs::write(dir.path().join("file_z.txt"), "").expect("write");
        std::fs::create_dir(dir.path().join("dir_z")).expect("mkdir");
        std::fs::write(dir.path().join("file_a.txt"), "").expect("write");
        std::fs::create_dir(dir.path().join("dir_a")).expect("mkdir");

        let entries = scan_directory(dir.path(), false).expect("scan");
        assert_eq!(entries.len(), 4);
        assert_eq!(entries[0].kind, EntryKind::Directory);
        assert_eq!(entries[0].name, "dir_a");
        assert_eq!(entries[1].kind, EntryKind::Directory);
        assert_eq!(entries[1].name, "dir_z");
        assert_eq!(entries[2].kind, EntryKind::File);
        assert_eq!(entries[2].name, "file_a.txt");
        assert_eq!(entries[3].kind, EntryKind::File);
        assert_eq!(entries[3].name, "file_z.txt");
    }

    #[test]
    fn test_scan_directory_only_hidden_files_returns_empty() {
        let dir = TempDir::new().expect("create temp dir");
        std::fs::write(dir.path().join(".gitignore"), "").expect("write");
        std::fs::create_dir(dir.path().join(".vscode")).expect("mkdir");

        let entries = scan_directory(dir.path(), false).expect("scan");
        assert!(entries.is_empty());
    }

    #[test]
    fn test_apply_fs_event_created_sorted_among_existing() {
        let dir = TempDir::new().expect("create temp dir");
        let file_a = dir.path().join("aaa.txt");
        let file_c = dir.path().join("ccc.txt");
        let file_b = dir.path().join("bbb.txt");
        std::fs::write(&file_a, "").expect("write");
        std::fs::write(&file_c, "").expect("write");
        std::fs::write(&file_b, "").expect("write");

        let mut roots = vec![FolderRoot {
            path: dir.path().to_path_buf(),
            entries: vec![
                TreeEntry {
                    name: "aaa.txt".to_string(),
                    path: file_a,
                    kind: EntryKind::File,
                    expanded: false,
                    children: Vec::new(),
                },
                TreeEntry {
                    name: "ccc.txt".to_string(),
                    path: file_c,
                    kind: EntryKind::File,
                    expanded: false,
                    children: Vec::new(),
                },
            ],
            expanded: true,
        }];

        apply_fs_event(&mut roots, &FsEvent::Created(file_b), false);
        assert_eq!(roots[0].entries.len(), 3);
        assert_eq!(roots[0].entries[0].name, "aaa.txt");
        assert_eq!(roots[0].entries[1].name, "bbb.txt");
        assert_eq!(roots[0].entries[2].name, "ccc.txt");
    }

    #[test]
    fn test_apply_fs_event_removed_last_entry() {
        let mut roots = vec![FolderRoot {
            path: PathBuf::from("/project"),
            entries: vec![TreeEntry {
                name: "only.rs".to_string(),
                path: PathBuf::from("/project/only.rs"),
                kind: EntryKind::File,
                expanded: false,
                children: Vec::new(),
            }],
            expanded: true,
        }];

        apply_fs_event(
            &mut roots,
            &FsEvent::Removed(PathBuf::from("/project/only.rs")),
            false,
        );
        assert!(roots[0].entries.is_empty());
    }

    #[test]
    fn test_apply_multiple_events_sequence() {
        let dir = TempDir::new().expect("create temp dir");
        let file_a = dir.path().join("a.txt");
        let file_b = dir.path().join("b.txt");
        std::fs::write(&file_a, "").expect("write");
        std::fs::write(&file_b, "").expect("write");

        let mut roots = vec![FolderRoot {
            path: dir.path().to_path_buf(),
            entries: Vec::new(),
            expanded: true,
        }];

        // Create two files
        apply_fs_event(&mut roots, &FsEvent::Created(file_a.clone()), false);
        apply_fs_event(&mut roots, &FsEvent::Created(file_b.clone()), false);
        assert_eq!(roots[0].entries.len(), 2);

        // Remove one
        apply_fs_event(&mut roots, &FsEvent::Removed(file_a), false);
        assert_eq!(roots[0].entries.len(), 1);
        assert_eq!(roots[0].entries[0].name, "b.txt");

        // Modified on existing is noop
        apply_fs_event(&mut roots, &FsEvent::Modified(file_b), false);
        assert_eq!(roots[0].entries.len(), 1);
    }

    #[test]
    fn test_find_parent_entries_empty_roots() {
        let mut roots: Vec<FolderRoot> = Vec::new();
        let result = find_parent_entries(&mut roots, &PathBuf::from("/project/file.txt"));
        assert!(result.is_none());
    }

    #[test]
    fn test_sort_entries_empty_slice() {
        let mut entries: Vec<TreeEntry> = Vec::new();
        sort_entries(&mut entries);
        assert!(entries.is_empty());
    }

    #[test]
    fn test_sort_entries_single_item() {
        let mut entries = vec![TreeEntry {
            name: "only.txt".to_string(),
            path: PathBuf::from("/only.txt"),
            kind: EntryKind::File,
            expanded: false,
            children: Vec::new(),
        }];
        sort_entries(&mut entries);
        assert_eq!(entries[0].name, "only.txt");
    }

    #[test]
    fn test_is_hidden_dot_prefix() {
        let dir = TempDir::new().expect("create temp dir");
        let hidden = dir.path().join(".hidden");
        std::fs::write(&hidden, "").expect("write");
        assert!(is_hidden(".hidden", &hidden));
    }

    #[test]
    fn test_is_hidden_normal_file() {
        let dir = TempDir::new().expect("create temp dir");
        let visible = dir.path().join("visible.txt");
        std::fs::write(&visible, "").expect("write");
        assert!(!is_hidden("visible.txt", &visible));
    }

    /// Builds a single expanded folder root over `path`, lazily empty.
    fn empty_root(path: &Path) -> Vec<FolderRoot> {
        vec![FolderRoot {
            path: path.to_path_buf(),
            entries: Vec::new(),
            expanded: true,
        }]
    }

    #[test]
    fn test_reconcile_directory_adds_new_entry() {
        let dir = TempDir::new().expect("create temp dir");
        std::fs::write(dir.path().join("old.txt"), "").expect("write");
        let mut entries = scan_directory(dir.path(), false).expect("scan");
        assert_eq!(entries.len(), 1);

        // A new file appears on disk after the initial scan.
        std::fs::write(dir.path().join("new.txt"), "").expect("write");
        let summary = reconcile_directory(dir.path(), &mut entries, false).expect("reconcile");

        assert_eq!(
            summary,
            ReconcileSummary {
                added: 1,
                removed: 0
            }
        );
        assert_eq!(entries.len(), 2);
        assert!(entries.iter().any(|e| e.name == "new.txt"));
    }

    #[test]
    fn test_reconcile_directory_removes_vanished_entry() {
        let dir = TempDir::new().expect("create temp dir");
        let gone = dir.path().join("gone.txt");
        std::fs::write(&gone, "").expect("write");
        std::fs::write(dir.path().join("keep.txt"), "").expect("write");
        let mut entries = scan_directory(dir.path(), false).expect("scan");
        assert_eq!(entries.len(), 2);

        std::fs::remove_file(&gone).expect("remove");
        let summary = reconcile_directory(dir.path(), &mut entries, false).expect("reconcile");

        assert_eq!(
            summary,
            ReconcileSummary {
                added: 0,
                removed: 1
            }
        );
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "keep.txt");
    }

    #[test]
    fn test_reconcile_directory_preserves_expansion_and_children() {
        let dir = TempDir::new().expect("create temp dir");
        let sub = dir.path().join("sub");
        std::fs::create_dir(&sub).expect("mkdir");
        let mut entries = scan_directory(dir.path(), false).expect("scan");

        // Mark the subdir expanded with a loaded (synthetic) child.
        entries[0].expanded = true;
        entries[0].children.push(TreeEntry {
            name: "loaded.rs".to_string(),
            path: sub.join("loaded.rs"),
            kind: EntryKind::File,
            expanded: false,
            children: Vec::new(),
        });

        // Add a sibling on disk, then reconcile the parent.
        std::fs::write(dir.path().join("sibling.txt"), "").expect("write");
        reconcile_directory(dir.path(), &mut entries, false).expect("reconcile");

        let sub_entry = entries
            .iter()
            .find(|e| e.name == "sub")
            .expect("sub survives");
        assert!(sub_entry.expanded, "expansion flag preserved");
        assert_eq!(sub_entry.children.len(), 1, "loaded children preserved");
        assert_eq!(sub_entry.children[0].name, "loaded.rs");
    }

    #[test]
    fn test_reconcile_directory_honours_hidden_flag() {
        let dir = TempDir::new().expect("create temp dir");
        std::fs::write(dir.path().join("visible.txt"), "").expect("write");
        let mut entries = scan_directory(dir.path(), false).expect("scan");

        std::fs::write(dir.path().join(".secret"), "").expect("write");
        reconcile_directory(dir.path(), &mut entries, false).expect("reconcile");
        assert!(
            !entries.iter().any(|e| e.name == ".secret"),
            "hidden entry must not surface when show_hidden is false"
        );

        reconcile_directory(dir.path(), &mut entries, true).expect("reconcile");
        assert!(
            entries.iter().any(|e| e.name == ".secret"),
            "hidden entry appears when show_hidden is true"
        );
    }

    #[test]
    fn test_reconcile_tree_recursive_picks_up_nested_new_dir() {
        // The Phase-20 Stage-2 regression: a folder created inside an
        // already-expanded subdirectory must surface after a recursive reload.
        let dir = TempDir::new().expect("create temp dir");
        let sub = dir.path().join("sub");
        std::fs::create_dir(&sub).expect("mkdir sub");

        let mut entries = scan_directory(dir.path(), false).expect("scan");
        // Expand `sub` and load its (currently empty) children.
        entries[0].expanded = true;
        entries[0].children = scan_directory(&sub, false).expect("scan sub");
        assert!(entries[0].children.is_empty());

        // A new nested folder appears on disk inside the expanded subdir.
        std::fs::create_dir(sub.join("nested")).expect("mkdir nested");
        let summary = reconcile_tree_recursive(dir.path(), &mut entries, false);

        assert_eq!(summary.added, 1, "nested addition counted");
        assert_eq!(entries[0].children.len(), 1);
        assert_eq!(entries[0].children[0].name, "nested");
    }

    #[test]
    fn test_reconcile_tree_recursive_skips_collapsed_dirs() {
        let dir = TempDir::new().expect("create temp dir");
        let sub = dir.path().join("sub");
        std::fs::create_dir(&sub).expect("mkdir");
        let mut entries = scan_directory(dir.path(), false).expect("scan");
        // Collapsed: children remain unloaded, recursion must not descend.
        entries[0].expanded = false;

        std::fs::create_dir(sub.join("nested")).expect("mkdir nested");
        reconcile_tree_recursive(dir.path(), &mut entries, false);

        assert!(
            entries[0].children.is_empty(),
            "collapsed directory must not be eagerly loaded"
        );
    }

    #[test]
    fn test_find_entry_mut_locates_nested_node() {
        let mut entries = vec![TreeEntry {
            name: "src".to_string(),
            path: PathBuf::from("/p/src"),
            kind: EntryKind::Directory,
            expanded: true,
            children: vec![TreeEntry {
                name: "main.rs".to_string(),
                path: PathBuf::from("/p/src/main.rs"),
                kind: EntryKind::File,
                expanded: false,
                children: Vec::new(),
            }],
        }];

        let hit = find_entry_mut(&mut entries, Path::new("/p/src/main.rs"));
        assert!(hit.is_some());
        assert_eq!(hit.expect("found").name, "main.rs");

        assert!(find_entry_mut(&mut entries, Path::new("/p/missing")).is_none());
    }

    #[test]
    fn test_apply_fs_event_dir_modified_reconciles_new_child() {
        // Reproduces the macOS FSEvents behaviour: a directory-level Modified
        // event (not a leaf event) must surface a newly created child.
        let dir = TempDir::new().expect("create temp dir");
        let sub = dir.path().join("sub");
        std::fs::create_dir(&sub).expect("mkdir");

        let mut roots = empty_root(dir.path());
        roots[0].entries.push(TreeEntry {
            name: "sub".to_string(),
            path: sub.clone(),
            kind: EntryKind::Directory,
            expanded: true,
            children: Vec::new(),
        });

        // New child created on disk; only a coalesced dir-level event arrives.
        std::fs::write(sub.join("late.txt"), "").expect("write");
        apply_fs_event(&mut roots, &FsEvent::Modified(sub.clone()), false);

        assert_eq!(roots[0].entries[0].children.len(), 1);
        assert_eq!(roots[0].entries[0].children[0].name, "late.txt");
    }

    #[test]
    fn test_apply_fs_event_new_dir_still_inserts_via_leaf_path() {
        // A brand-new directory (leaf event, Windows/Linux style) is not yet
        // in the tree, so it must take the insert fast-path, not reconcile.
        let dir = TempDir::new().expect("create temp dir");
        let mut roots = empty_root(dir.path());

        let new_dir = dir.path().join("fresh");
        std::fs::create_dir(&new_dir).expect("mkdir");
        apply_fs_event(&mut roots, &FsEvent::Created(new_dir.clone()), false);

        assert_eq!(roots[0].entries.len(), 1);
        assert_eq!(roots[0].entries[0].name, "fresh");
        assert_eq!(roots[0].entries[0].kind, EntryKind::Directory);
    }

    #[cfg(windows)]
    #[test]
    fn test_is_hidden_windows_attribute() {
        let dir = TempDir::new().expect("create temp dir");
        let path = dir.path().join("win_hidden.txt");
        std::fs::write(&path, "").expect("write");

        // Set FILE_ATTRIBUTE_HIDDEN via `attrib`
        std::process::Command::new("attrib")
            .args(["+h", &path.to_string_lossy()])
            .status()
            .expect("attrib +h");

        assert!(
            is_hidden("win_hidden.txt", &path),
            "File with FILE_ATTRIBUTE_HIDDEN should be detected as hidden"
        );
    }

    #[cfg(windows)]
    #[test]
    fn test_scan_directory_skips_windows_hidden_attribute() {
        let dir = TempDir::new().expect("create temp dir");
        let hidden = dir.path().join("win_hidden.txt");
        let visible = dir.path().join("visible.txt");
        std::fs::write(&hidden, "").expect("write");
        std::fs::write(&visible, "").expect("write");

        // Set FILE_ATTRIBUTE_HIDDEN on the file
        std::process::Command::new("attrib")
            .args(["+h", &hidden.to_string_lossy()])
            .status()
            .expect("attrib +h");

        let entries = scan_directory(dir.path(), false).expect("scan");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "visible.txt");

        // With show_hidden=true, both should appear
        let entries = scan_directory(dir.path(), true).expect("scan");
        assert_eq!(entries.len(), 2);
    }
}
