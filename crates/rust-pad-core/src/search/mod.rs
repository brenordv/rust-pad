mod finder;
mod folder;

pub use finder::{SearchEngine, SearchMatch, SearchOptions};
pub use folder::{search_folder, FolderSearchHit, FolderSearchLimits, FolderSearchOutcome};
