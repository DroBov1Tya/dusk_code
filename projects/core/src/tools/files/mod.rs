//! File tools: read, write, edit and delete files inside the tool scope.

mod delete;
mod edit;
mod read;
mod search;
mod text;
mod write;

pub use delete::DeleteFile;
pub use edit::EditFile;
pub use read::ReadFile;
pub use search::SearchFiles;
pub use text::{PathArguments, read_text, write_text};
use text::{skill_save, with_skill_save};
pub use write::WriteFile;
