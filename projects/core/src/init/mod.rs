//! First-start initialization. Creates the missing parts of the layout in `paths`
//! with private permissions and never modifies existing files.

pub mod logs;
pub mod paths;
mod setup;

pub use setup::{Initialization, create_private_dir, initialize};
