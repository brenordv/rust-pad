/// Undo/redo history management with tiered storage.
///
/// Provides an `UndoManager` that keeps recent edit groups in memory
/// and spills older groups to an embedded key-value store (redb) on disk.
/// History is persisted per-document and survives across application sessions.
pub mod config;
pub mod manager;
pub mod operation;
pub mod persistence;

pub use config::HistoryConfig;
pub use manager::UndoManager;
pub use operation::{CursorSnapshot, EditGroup, EditOperation};
pub use persistence::PersistenceLayer;
