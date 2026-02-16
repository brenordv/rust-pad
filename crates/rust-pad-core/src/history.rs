// Re-exports from rust-pad-mod-history and conversion traits.
// Bridges the history crate's types with rust-pad-core's Position type.
pub use rust_pad_mod_history::config::{doc_id_for_path, generate_unsaved_id};
pub use rust_pad_mod_history::{
    CursorSnapshot, EditGroup, EditOperation, HistoryConfig, PersistenceLayer, UndoManager,
};

use crate::cursor::Position;

impl From<CursorSnapshot> for Position {
    fn from(s: CursorSnapshot) -> Self {
        Position {
            line: s.line,
            col: s.col,
        }
    }
}
