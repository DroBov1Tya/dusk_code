//! The project graph: the structure of a saved project's source, never its text. `parse`
//! turns one file into the names it defines and the names it refers to, under limits that
//! hold for any input; `languages` says how each language is read; `outline` is what a
//! parse gives, and the kinds the store keeps. `index` is the worker that keeps the graph
//! of saved projects current and `refresh` what it does with one project. Design:
//! `docs/project-intelligence.md`.

mod index;
#[cfg(test)]
mod index_tests;
mod languages;
mod outline;
mod parse;
mod refresh;
#[cfg(test)]
mod tests;

// `Status` is what the `project_graph` tool reads (task G4 of the design).
#[allow(unused_imports)]
pub use index::Status;
pub use index::{Changed, Folder, Indexer};
pub use languages::Language;
pub use outline::{ParsedFile, RefKind, Skipped, SymbolKind};
pub use parse::{MAX_FILE_BYTES, parse};
