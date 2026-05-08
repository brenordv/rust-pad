/// Workspace sidebar: file explorer with named workspaces.
///
/// Modules:
/// - `watcher`: Filesystem monitoring via `notify` crate.
/// - `tree`: Tree data structures for the sidebar.
/// - `scanner`: Directory scanning and tree updates.
/// - `sidebar`: UI rendering of the sidebar panel.
pub mod scanner;
pub mod sidebar;
pub mod tree;
pub mod watcher;

pub use sidebar::{SidebarAction, WorkspaceSidebar};
pub use tree::{EntryKind, FolderRoot, TreeEntry};
pub use watcher::{FsEvent, WorkspaceWatcher};
