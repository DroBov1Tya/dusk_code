//! The machine dusk runs on, found first when the core starts. Each system's own code
//! lives in its folder and is built only there; this level picks the one that applies.
//! `notify` posts a notice and a sound; `usage` measures what the app costs the machine.

mod detect;
#[cfg(target_os = "linux")]
mod linux;
pub mod login_path;
#[cfg(target_os = "macos")]
mod macos;
pub mod notify;
mod sandbox;
pub mod usage;

pub use detect::{Needs, Platform};
#[cfg(target_os = "macos")]
pub use macos::simulator;
pub use sandbox::{
    Confinement, Network, available as sandbox_available, confined_program, shell, temp_folders,
};
