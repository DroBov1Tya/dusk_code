//! What the process and its data take on this machine.

use serde::{Deserialize, Serialize};

/// What the application costs the machine right now.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SystemReport {
    pub process: Option<ProcessUsage>,
    /// The database file, its write-ahead log and shared memory.
    pub database_bytes: u64,
    pub wal_bytes: u64,
    /// Images kept for image cards.
    pub artifacts_bytes: u64,
    pub logs_bytes: u64,
    pub threads: u64,
    pub messages: u64,
}

/// The running process as `ps` sees it; `None` where `ps` is missing.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ProcessUsage {
    pub resident_bytes: u64,
    /// Of one core, averaged over the process lifetime on macOS and recently on Linux.
    pub cpu_percent: f32,
}
