//! File tools: read and search files inside the tool scope, and ask the change ledger
//! what commands changed. Writing goes through `run_command`, which the sandbox confines
//! to the same folders.

mod changes;
mod read;
mod search;
mod text;

pub use changes::Changes;
pub use read::ReadFile;
pub use search::SearchFiles;
pub use text::read_text;
