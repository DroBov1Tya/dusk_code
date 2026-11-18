//! The project graph: the structure of a saved project's source, never its text. `parse`
//! turns one file into the names it defines and the names it refers to, under limits that
//! hold for any input; `languages` says how each language is read; `outline` is what a
//! parse gives. Design: `docs/project-intelligence.md`.

// Nothing calls the parser yet: the store and the indexer (tasks G2 and G3 of the design)
// are its first users. Remove this with the indexer.
#![allow(dead_code, unused_imports)]

mod languages;
mod outline;
mod parse;
#[cfg(test)]
mod tests;

pub use languages::Language;
pub use outline::{
    ImportPath, MAX_IMPORT_PATH_CHARS, MAX_NAME_CHARS, Name, Outline, ParsedFile, Ref, RefKind,
    Skipped, Symbol, SymbolKind,
};
pub use parse::{MAX_FILE_BYTES, parse};
