//! A background command of a thread.

use serde::{Deserialize, Serialize};

/// A job as a client lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobInfo {
    pub job_id: String,
    pub command: String,
    /// In words: running, completed, failed, cancelled, timed_out, interrupted, or running
    /// in, or unknown because of, another runtime.
    pub status: String,
    pub running: bool,
    /// This runtime owns its process, so it can be stopped from here.
    pub stoppable: bool,
    pub started_at: i64,
    pub finished_at: Option<i64>,
}
