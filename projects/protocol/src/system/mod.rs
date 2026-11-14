//! What the process and its data take on this machine, and its clock.

mod clock;
mod report;
mod simulator;

#[cfg(not(target_arch = "wasm32"))]
pub use clock::now_ms;
pub use report::{ProcessUsage, SystemReport};
pub use simulator::{SimButton, SimDevice, SimScreen, Simulator};
