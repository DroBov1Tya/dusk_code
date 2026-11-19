//! The local API and the core shared by the apps of one user. The first app takes the lock
//! and serves the core on a private socket; the apps after it attach there and speak the
//! same stream as the browser's.
//!
//! - `files`: where the run folder, the socket and the key are, and the key itself;
//! - `lock`: the lock the owner holds for the life of the core, and what it says;
//! - `lines`: the JSON lines transport of the local API.

#[cfg(unix)]
mod files;
pub mod lines;
#[cfg(unix)]
mod lock;
#[cfg(all(test, unix))]
mod tests;

#[cfg(unix)]
pub use files::{RunFiles, read_key, write_key};
#[cfg(unix)]
pub use lock::{CoreLock, LockInfo, read_info};
