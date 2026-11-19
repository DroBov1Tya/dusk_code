//! Console tools: commands in the working directory, and the background ones.

mod file_changes;
mod jobs;
pub mod process;
mod run_command;
mod shape;

pub use file_changes::skipped_folders;
pub use jobs::Jobs;
pub use run_command::{RunCommand, run_check, run_for_user};
pub use shape::Shaped;
