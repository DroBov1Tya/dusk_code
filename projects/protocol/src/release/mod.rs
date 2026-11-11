//! Release channels, the newest release and how far an install got.

mod channel;
mod info;
mod install;

pub use channel::Channel;
pub use info::UpdateInfo;
pub use install::{InstallProgress, InstallStage};
