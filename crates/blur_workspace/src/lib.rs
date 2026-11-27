//! Project roots, directory listing, file indexing + fuzzy finding, project search & replace,
//! file watching, workspace persistence/snapshots/recents, task detection/running, and test parsing.
//!
//! This crate is GPUI-free pure Rust.

pub mod index;
pub mod persistence;
pub mod project;
pub mod search;
pub mod tasks;
pub mod watcher;

pub use index::{FileIndex, FuzzyMatch, IndexedFile};
pub use project::{list_dir, DirEntry, ListOptions, Project};
pub use search::{replace_in_file, search, FileMatches, LineMatch, SearchQuery, SearchResults};
pub use watcher::{FileWatcher, FsEvent, FsEventKind};
