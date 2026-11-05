//! The machine dusk runs on, found first when the core starts. Each system's own code
//! lives in its folder and is built only there; this level picks the one that applies.

mod detect;
#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
mod sandbox;

pub use detect::{Needs, Platform};
pub use sandbox::{Confinement, shell};
