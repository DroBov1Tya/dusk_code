//! A thread's background jobs, sources, plan, team and changes.

use serde::{Deserialize, Serialize};

/// The largest picture a client may send as bytes. A browser's frame carries the bytes
/// as JSON numbers, about four characters each, so this keeps it under the web's frame cap.
pub const MAX_PICTURE_UPLOAD: usize = 6 * 1024 * 1024;

/// A thread's background jobs, sources, plan, team and changes. The plan and team
/// requests name the root thread, which leads the team.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
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
    /// A PNG or JPEG of up to `MAX_PICTURE_UPLOAD` bytes, sent by a client that cannot
    /// name a file on this machine, such as a browser; kept among the thread's sources.
    /// Answers with the tag that puts it in a message.
    UploadPicture {
        thread_id: String,
        name: String,
        bytes: Vec<u8>,
    },
    /// The bytes of the thread's picture source `name`, as its tag names it, for a
    /// client to draw. Answers with `Response::Picture`; a source that is no PNG or
    /// JPEG is refused.
    SourcePicture {
        thread_id: String,
        name: String,
    },
    /// A part of the thread's source `source_id` for a client to read, from byte
    /// `offset`. Answers with `Response::SourceText`.
    SourceText {
        thread_id: String,
        source_id: String,
        offset: u64,
    },
    /// A part of a card a message of the thread, or of an agent thread below it, carries:
    /// `image`, `audio` or `thumb`. Answers with `Response::Picture`, or an error when the
    /// thread has no such card.
    Card {
        thread_id: String,
        card_id: String,
        part: String,
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
    /// What the chat's commands changed in its project, file by file with diffs, newest
    /// first, at most `MAX_FILE_CHANGES`.
    FileChanges {
        thread_id: String,
    },
    /// The plan for the strip: every task not done, then up to `limit` done ones older
    /// than the task `done_before`, newest first; the core holds `limit` to
    /// `PLAN_PAGE_DONE`. Answers with `Response::PlanPage`.
    PlanTasks {
        thread_id: String,
        done_before: Option<String>,
        limit: u32,
    },
    /// The team running the plan. Answers with `Response::Team`.
    Team {
        thread_id: String,
    },
    /// How the thread's work stands: its activity, changes and newest log lines. Answers
    /// with `Response::Overview`.
    Overview {
        thread_id: String,
    },
    /// The thread's event log older than `before` (Unix ms), newest first; the core holds
    /// `limit` to `EVENTS_PAGE`. Answers with `Response::Events`.
    Events {
        thread_id: String,
        before: Option<i64>,
        limit: u32,
    },
    /// Passes the `user_confirmation` gate of a task in review.
    ConfirmTask {
        thread_id: String,
        task_id: String,
    },
    /// Ends the running team as stopped.
    StopTeam {
        thread_id: String,
    },
    /// Sets how many members of the running team work at once, 1 to the setting's
    /// `team_at_once`.
    SetTeamAtOnce {
        thread_id: String,
        at_once: u32,
    },
}
