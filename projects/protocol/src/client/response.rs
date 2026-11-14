//! What the core answers to a request.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::catalog::NodeText;
use crate::client::Answer;
use crate::config::{ApprovalPolicy, McpInfo, Profile, SecretsInfo, Snippet, VoiceSettings};
use crate::project::{
    Change, FileDiff, FileEntry, FileStamp, FileText, GitInfo, OutputFile, Project, Schedule,
};
use crate::system::SystemReport;
use crate::thread::{Found, JobInfo, LocalUnsent, Message, Task, Thread};
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
    LocalUnsent(Vec<LocalUnsent>),
    Jobs(Vec<JobInfo>),
    Node(NodeText),
    Mcp(Vec<McpInfo>),
    Snippets(Vec<Snippet>),
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
    /// A sound or picture card the core made.
    Card(crate::tools::Card),
    /// A thread's own approval mode, or none when it uses the default in settings.
    Approvals(Option<ApprovalPolicy>),
    Projects(Vec<Project>),
    Schedules(Vec<Schedule>),
    Project(Project),
    Git(GitInfo),
    Diff(Vec<FileDiff>),
    Files(Vec<FileEntry>),
    Changes(Vec<Change>),
    Outputs(Vec<OutputFile>),
    /// A file read for the editor.
    File(FileText),
    /// What a file is on disk after a write.
    Stamp(FileStamp),
    System(SystemReport),
    Simulator(crate::system::Simulator),
    /// A JPEG: a simulated device's screen.
    Picture(Vec<u8>),
    Answer(Box<Answer>),
    /// The request failed; the text says why in full.
    Error(String),
}
