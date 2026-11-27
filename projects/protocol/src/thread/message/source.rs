//! A file kept among a thread's sources.

use serde::{Deserialize, Serialize};

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Source {
    pub source_id: String,
    pub name: String,
    pub file: String,
    pub bytes: u64,
    pub pinned: bool,
    pub delivered: bool,
    /// A plan the plan tool or the user's plan editor wrote, rather than an attached file.
    #[serde(default)]
    pub plan: bool,
}

/// One of a thread's plans, as `WorkRequest::Plans` lists them, newest written first.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanFile {
    /// The source it is kept as; `WorkRequest::RemoveSource` deletes it.
    pub source_id: String,
    pub name: String,
    pub bytes: u64,
    /// When it was last written, Unix ms.
    pub written_at: i64,
    /// The plan the Plan screen shows and edits: the one written last.
    pub current: bool,
    /// Every checkbox of it is ticked, and it has one at least.
    pub finished: bool,
}

impl PlanFile {
    /// `WorkRequest::RemoveOldPlans` deletes it: a plan replaced by a newer one, or done.
    pub fn old(&self) -> bool {
        !self.current || self.finished
    }
}
