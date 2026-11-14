//! First-start initialization. Creates the missing parts of the layout in `paths`
//! with private permissions and never modifies existing files. `files` is the private-file
//! boundary every record goes through: atomic writes and reads that follow no link.

pub mod files;
pub mod logs;
pub mod paths;
mod setup;

pub use setup::{create_private_dir, initialize};
