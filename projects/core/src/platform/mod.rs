//! The machine dusk runs on, found first when the core starts. Each system's own code
//! lives in its folder and is built only there; this level picks the one that applies.

mod detect;
#[cfg(target_os = "linux")]
mod linux;
pub mod login_path;
#[cfg(target_os = "macos")]
mod macos;
mod sandbox;

pub use detect::{Needs, Platform};
#[cfg(target_os = "macos")]
pub use macos::simulator;
pub use sandbox::{Confinement, shell};
