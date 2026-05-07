/// Tree data structures for the workspace sidebar.
use std::path::PathBuf;

/// The kind of entry in the file tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    File,
    Directory,
}

/// A single entry (file or directory) in the tree.
#[derive(Debug, Clone)]
pub struct TreeEntry {
    /// Display name (file/dir name only, not full path).
    pub name: String,
    /// Full absolute path.
    pub path: PathBuf,
    /// Whether this is a file or directory.
    pub kind: EntryKind,
    /// Whether this directory is expanded (only meaningful for directories).
    pub expanded: bool,
    /// Children entries (lazy-loaded on expand).
    pub children: Vec<TreeEntry>,
}

/// A root folder in the workspace.
#[derive(Debug, Clone)]
pub struct FolderRoot {
    /// Absolute path of this root folder.
    pub path: PathBuf,
    /// Top-level entries in this folder.
    pub entries: Vec<TreeEntry>,
    /// Whether this root is expanded in the UI.
    pub expanded: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_entry_kind_copy_and_eq() {
        let file = EntryKind::File;
        let dir = EntryKind::Directory;
        let file2 = file;
        assert_eq!(file, file2);
        assert_ne!(file, dir);
    }

    #[test]
    fn test_entry_kind_debug() {
        assert_eq!(format!("{:?}", EntryKind::File), "File");
        assert_eq!(format!("{:?}", EntryKind::Directory), "Directory");
    }

    #[test]
    fn test_tree_entry_creation() {
        let entry = TreeEntry {
            name: "main.rs".to_string(),
            path: PathBuf::from("/project/src/main.rs"),
            kind: EntryKind::File,
            expanded: false,
            children: Vec::new(),
        };
        assert_eq!(entry.name, "main.rs");
        assert_eq!(entry.kind, EntryKind::File);
        assert!(!entry.expanded);
        assert!(entry.children.is_empty());
    }

    #[test]
    fn test_tree_entry_with_children() {
        let child = TreeEntry {
            name: "lib.rs".to_string(),
            path: PathBuf::from("/project/src/lib.rs"),
            kind: EntryKind::File,
            expanded: false,
            children: Vec::new(),
        };
        let dir = TreeEntry {
            name: "src".to_string(),
            path: PathBuf::from("/project/src"),
            kind: EntryKind::Directory,
            expanded: true,
            children: vec![child],
        };
        assert_eq!(dir.kind, EntryKind::Directory);
        assert!(dir.expanded);
        assert_eq!(dir.children.len(), 1);
        assert_eq!(dir.children[0].name, "lib.rs");
    }

    #[test]
    fn test_tree_entry_clone() {
        let entry = TreeEntry {
            name: "test.rs".to_string(),
            path: PathBuf::from("/test.rs"),
            kind: EntryKind::File,
            expanded: false,
            children: Vec::new(),
        };
        let cloned = entry.clone();
        assert_eq!(entry.name, cloned.name);
        assert_eq!(entry.path, cloned.path);
        assert_eq!(entry.kind, cloned.kind);
    }

    #[test]
    fn test_folder_root_creation() {
        let root = FolderRoot {
            path: PathBuf::from("/project"),
            entries: Vec::new(),
            expanded: true,
        };
        assert_eq!(root.path, PathBuf::from("/project"));
        assert!(root.entries.is_empty());
        assert!(root.expanded);
    }

    #[test]
    fn test_folder_root_clone() {
        let root = FolderRoot {
            path: PathBuf::from("/project"),
            entries: vec![TreeEntry {
                name: "readme.md".to_string(),
                path: PathBuf::from("/project/readme.md"),
                kind: EntryKind::File,
                expanded: false,
                children: Vec::new(),
            }],
            expanded: true,
        };
        let cloned = root.clone();
        assert_eq!(root.path, cloned.path);
        assert_eq!(root.entries.len(), cloned.entries.len());
        assert_eq!(root.expanded, cloned.expanded);
    }

    #[test]
    fn test_folder_root_debug() {
        let root = FolderRoot {
            path: PathBuf::from("/test"),
            entries: Vec::new(),
            expanded: false,
        };
        let debug = format!("{root:?}");
        assert!(debug.contains("FolderRoot"));
        assert!(debug.contains("test"));
    }
}
