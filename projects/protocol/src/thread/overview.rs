//! The Overview of a thread: what its tools did, what it changed and its event log, as the
//! core counts them from what it keeps. Progress and members are the plan's and the team's
//! answers; tokens and context come with their own events.

use serde::{Deserialize, Serialize};

use crate::thread::Line;

/// The most used tools an `Overview` names.
pub const OVERVIEW_TOOLS: usize = 8;
/// The newest changed paths an `Overview` names.
pub const OVERVIEW_PATHS: usize = 5;
/// The newest log lines an `Overview` holds.
pub const OVERVIEW_LOG: usize = 20;
/// The most log lines one page of `WorkRequest::Events` holds.
pub const EVENTS_PAGE: u32 = 100;
/// The longest text of a log line.
pub const LOG_CHARS: usize = 160;

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Overview {
    #[serde(default)]
    pub activity: Activity,
    #[serde(default)]
    pub changes: ChangeSum,
    /// Newest first, at most `OVERVIEW_LOG`.
    #[serde(default)]
    pub log: Vec<LogEntry>,
}

/// What the thread's tools did, from every call it made.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Activity {
    pub tool_runs: u64,
    /// Most used first, at most `OVERVIEW_TOOLS`.
    pub by_tool: Vec<ToolRuns>,
    /// The subagents the thread asked.
    pub agents_asked: u32,
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolRuns {
    /// As the model called it: data, drawn as plain text.
    pub name: String,
    pub runs: u64,
}

/// What the thread changed in its project, from the change ledger.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangeSum {
    /// Distinct paths.
    pub files: u32,
    /// Lines, from the diffs that were kept.
    pub added: u64,
    pub removed: u64,
    /// Newest first, at most `OVERVIEW_PATHS`.
    pub newest: Vec<ChangedPath>,
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangedPath {
    /// Relative to the project folder.
    pub path: String,
    pub added: u64,
    pub removed: u64,
}

/// One line of a thread's event log, written by the core from what it stored.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogEntry {
    /// Unix milliseconds.
    pub at: i64,
    pub kind: LogKind,
    pub text: Line<LOG_CHARS>,
}

/// How a log line ended, which picks its glyph.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogKind {
    Started,
    Done,
    Stopped,
    Failed,
}
