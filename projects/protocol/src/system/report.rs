//! What the process and its data take on this machine.

use serde::{Deserialize, Serialize};

/// What the application costs the machine right now.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SystemReport {
    /// The app's own process: the window and the core inside it.
    pub process: Option<ProcessUsage>,
    /// The processes it started and theirs: MCP servers, plugin programs, commands,
    /// terminals; none while there are none or where `ps` is missing.
    #[serde(default)]
    pub children: Option<Children>,
    /// The database file, its write-ahead log and shared memory.
    pub database_bytes: u64,
    pub wal_bytes: u64,
    /// Images kept for image cards.
    pub artifacts_bytes: u64,
    pub logs_bytes: u64,
    pub threads: u64,
    pub messages: u64,
}

impl SystemReport {
    /// The app's process and its children together: what the app costs the machine.
    pub fn total(&self) -> Option<ProcessUsage> {
        let own = self.process?;
        let Some(children) = &self.children else {
            return Some(own);
        };
        Some(ProcessUsage {
            resident_bytes: own.resident_bytes + children.usage.resident_bytes,
            cpu_percent: own.cpu_percent + children.usage.cpu_percent,
        })
    }
}

/// The processes below the app's own, summed.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Children {
    pub count: u32,
    pub usage: ProcessUsage,
}

/// The running process as `ps` sees it; `None` where `ps` is missing.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ProcessUsage {
    pub resident_bytes: u64,
    /// Of one core, averaged over the process lifetime on macOS and recently on Linux.
    pub cpu_percent: f32,
}
