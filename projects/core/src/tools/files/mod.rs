//! File tools: read and search files inside the tool scope, ask the change ledger what
//! commands changed, and ask the project graph where names are defined and used. Writing
//! goes through `run_command`, which the sandbox confines to the same folders.

mod changes;
mod graph;
mod read;
mod search;
mod text;

pub use changes::Changes;
pub use graph::ProjectGraph;
pub use read::ReadFile;
pub use search::SearchFiles;
pub use text::read_text;
