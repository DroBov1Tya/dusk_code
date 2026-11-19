//! The core shared by the apps of one user. The first app takes the lock and serves the
//! core on a private socket; the apps after it attach there.
//!
//! - `files`: where the run folder, the socket and the key are, and the key itself;
//! - `lock`: the lock the owner holds for the life of the core, and what it says.

mod files;
mod lock;
#[cfg(test)]
mod tests;

pub use files::{RunFiles, read_key, write_key};
pub use lock::{CoreLock, LockInfo, read_info};
