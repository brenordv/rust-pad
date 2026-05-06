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
