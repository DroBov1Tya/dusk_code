//! What the agent remembers.

use serde::{Deserialize, Serialize};

/// Serde defaults of the settings below.
pub fn yes() -> bool {
    true
}

pub fn default_memory_chars() -> usize {
    2000
}

/// The memory of the user: whether the agent keeps it, and its size. Every request
/// carries it, so it is kept small.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Memory {
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default = "default_memory_chars")]
    pub max_chars: usize,
}

impl Default for Memory {
    fn default() -> Self {
        Self {
            enabled: true,
            max_chars: default_memory_chars(),
        }
    }
}

impl Memory {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

/// Compact facts about the current project, kept separately from user memory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkHistory {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_memory_chars")]
    pub max_chars: usize,
}

impl Default for WorkHistory {
    fn default() -> Self {
        Self {
            enabled: false,
            max_chars: default_memory_chars(),
        }
    }
}

impl WorkHistory {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}
