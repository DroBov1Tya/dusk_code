//! What the core answers to a request.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::catalog::NodeText;
use crate::client::Answer;
use crate::config::{ApprovalPolicy, McpInfo, Profile, SecretsInfo, Snippet, VoiceSettings};
use crate::project::{Change, FileDiff, FileEntry, GitInfo, OutputFile, Project};
use crate::system::SystemReport;
use crate::thread::{JobInfo, LocalUnsent, Message, Task, Thread};
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
    Project(Project),
    Git(GitInfo),
    Diff(Vec<FileDiff>),
    Files(Vec<FileEntry>),
    Changes(Vec<Change>),
    Outputs(Vec<OutputFile>),
    System(SystemReport),
    Answer(Box<Answer>),
    /// The request failed; the text says why in full.
    Error(String),
}
