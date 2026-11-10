//! File tools: read and search files inside the tool scope. Writing goes through
//! `run_command`, which the sandbox confines to the same folders.

mod read;
mod search;
mod text;

pub use read::ReadFile;
pub use search::SearchFiles;
pub use text::read_text;
