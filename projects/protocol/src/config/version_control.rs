//! How branches the agents make are named.

use serde::{Deserialize, Serialize};

/// Version control settings; clients show this section as "Version control".
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VersionControl {
    /// The prefix for branch names this tool creates, such as `dusk/`; empty adds none.
    #[serde(default)]
    pub branch_prefix: String,
}

impl VersionControl {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}
