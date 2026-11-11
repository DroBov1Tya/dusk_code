//! A thread's background jobs, sources, plan and changes.

use serde::{Deserialize, Serialize};

/// A thread's background jobs and sources.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum WorkRequest {
    Jobs {
        thread_id: String,
    },
    JobOutput {
        thread_id: String,
        job_id: String,
    },
    StopJob {
        thread_id: String,
        job_id: String,
    },
    PinSource {
        thread_id: String,
        id: String,
        pinned: bool,
    },
    RemoveSource {
        thread_id: String,
        id: String,
    },
    /// The chat's pinned plan, a Markdown text the user and the model both edit.
    Plan {
        thread_id: String,
    },
    /// Rewrites the plan; empty text removes it.
    SavePlan {
        thread_id: String,
        text: String,
    },
    /// The changes in the chat's own folder against the last commit, as `Diff` of a
    /// project answers; the folder is the one the chat started in.
    Diff {
        thread_id: String,
        path: Option<String>,
    },
    /// The files the chat's commands changed in its project.
    ChangedFiles {
        thread_id: String,
    },
}
