//! The summary that stands for older messages.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Compaction {
    pub compaction_id: i64,
    pub thread_id: String,
    /// The newest message the summary covers.
    pub through_sequence: u64,
    pub summary: String,
    /// Estimated size of the summary.
    pub summary_tokens: u64,
}
