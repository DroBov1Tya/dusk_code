//! What the core answers to a request.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::catalog::{NodeText, SkillPack};
use crate::client::Answer;
use crate::config::{ApprovalPolicy, McpInfo, Profile, SecretsInfo, Snippet, VoiceSettings};
use crate::project::{
    Change, FileChange, FileDiff, FileEntry, FileStamp, FileText, GitInfo, GitOutcome, OutputFile,
    Project, PullRequest, PullRequestDetail, Schedule,
};
use crate::system::SystemReport;
use crate::thread::{
    Found, JobInfo, LocalUnsent, LogEntry, Message, Overview, Pin, PlanPage, Task, Team, Thread,
};
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Response {
    Done,
    Opened {
        thread_id: String,
    },
    Waiting(bool),
    Text(String),
    Name(Option<String>),
    Number(u64),
    Path(PathBuf),
    Threads(Vec<Thread>),
    Messages(Vec<Message>),
    /// Messages of any chat that contain the searched text.
    Found(Vec<Found>),
    Tasks(Vec<Task>),
    /// A page of a root thread's plan.
    PlanPage(PlanPage),
    /// The team running a root thread's plan, or none before one ran.
    Team(Option<Team>),
    /// How a thread's work stands.
    Overview(Overview),
    /// A page of a thread's event log, newest first.
    Events(Vec<LogEntry>),
    /// A thread's pins, oldest message first.
    Pins(Vec<Pin>),
    LocalUnsent(Vec<LocalUnsent>),
    Jobs(Vec<JobInfo>),
    Node(NodeText),
    Mcp(Vec<McpInfo>),
    Snippets(Vec<Snippet>),
    SkillPacks(Vec<SkillPack>),
    /// Names, sorted: a project's environment variables.
    Names(Vec<String>),
    /// The pinned plan, or none before one was written.
    Plan(Option<String>),
    Profile(Profile),
    Secrets(SecretsInfo),
    Voice(VoiceSettings),
    VoiceModels(crate::config::VoiceModels),
    /// What a check of a provider found.
    Provider(crate::model::ProviderStatus),
    /// A PNG or JPEG of a thread's sources.
    Picture(Vec<u8>),
    /// A sound or picture card the core made.
    Card(crate::tools::Card),
    /// A thread's own approval mode, or none when it uses the default in settings.
    Approvals(Option<ApprovalPolicy>),
    Projects(Vec<Project>),
    Schedules(Vec<Schedule>),
    Project(Project),
    Git(GitInfo),
    /// What a change of a repository left.
    GitDone(GitOutcome),
    PullRequests(Vec<PullRequest>),
    PullRequest(Box<PullRequestDetail>),
    Diff(Vec<FileDiff>),
    Files(Vec<FileEntry>),
    Changes(Vec<Change>),
    /// A thread's change ledger, newest first.
    FileChanges(Vec<FileChange>),
    Outputs(Vec<OutputFile>),
    /// A file read for the editor.
    File(FileText),
    /// What a file is on disk after a write.
    Stamp(FileStamp),
    System(SystemReport),
    Answer(Box<Answer>),
    Store(crate::plugin::StoreList),
    StoreUpdates(crate::plugin::UpdateReport),
    /// A store picture (a cover or a screenshot), checked against its list: its bytes in
    /// base64, and their type.
    Media {
        base64: String,
        mime: String,
    },
    /// The request failed; the text says why in full.
    Error(String),
}
