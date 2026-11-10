//! What a client asks the core, what the core answers, the events it pushes and the
//! status a client draws from. A client says hello first; every request then gets
//! exactly one response, while events and status changes arrive on their own.

use std::{fmt, path::PathBuf};

use serde::{Deserialize, Serialize};

use crate::{
    catalog::NodeText,
    command::{Answer, Command},
    config::{ApprovalPolicy, ContextPolicy, ProviderKind},
    conversation::{LocalUnsent, Message},
    mcp::{McpInfo, McpStatus},
    mentions::{Profile, Snippet},
    model::Effort,
    plugins::PluginInfo,
    project::{Change, FileDiff, FileEntry, GitInfo, OutputFile, Project},
    system::SystemReport,
    thread::{Thread, ThreadEvent},
    tools::Decision,
    update::InstallProgress,
    work::{JobInfo, Task},
};

/// Raised whenever a request, a response or an event changes shape.
pub const PROTOCOL_VERSION: u32 = 3;

/// The first thing a client says.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hello {
    pub protocol: u32,
    /// Who is asking, such as `tui`; threads it creates carry it as their source.
    pub client: String,
}

/// A value that must not reach a log: it prints as hidden.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Secret(pub String);

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(hidden)")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Request {
    Thread(ThreadRequest),
    Settings(SettingsRequest),
    Catalog(CatalogRequest),
    Work(WorkRequest),
    Project(ProjectRequest),
    /// A slash command that does not run in a thread.
    Command(Command),
    SystemReport,
    /// Installs release `tag`; progress shows in the status until it is done.
    InstallUpdate {
        tag: String,
    },
    /// Closes every thread and stops the core; the last request a client sends.
    Shutdown,
}

/// Opening, feeding and closing threads. Events of an open thread arrive as
/// `Event::Thread` until `Event::Ended`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ThreadRequest {
    New {
        cwd: PathBuf,
    },
    Resume {
        thread_id: String,
        cwd: PathBuf,
    },
    /// A new thread that starts from a /fork summary.
    Fork {
        summary: String,
        cwd: PathBuf,
    },
    List,
    Messages {
        thread_id: String,
    },
    Tasks {
        thread_id: String,
    },
    LocalUnsent {
        thread_id: String,
    },
    ResendLocalUnsent {
        thread_id: String,
        id: String,
    },
    /// Keeps long pastes and named pictures among the thread's sources, then queues the
    /// message; `now` asks for it before what is queued.
    Send {
        thread_id: String,
        text: String,
        now: bool,
    },
    Cancel {
        thread_id: String,
    },
    /// Sends a command the agent runs in the foreground, by its tool call, to the
    /// background: the agent goes on and gets the result when it ends.
    Background {
        thread_id: String,
        call_id: String,
    },
    /// Runs a command the user confirmed, in the chat's folder; answers with its output.
    RunCommand {
        thread_id: String,
        command: String,
    },
    /// Stops the thread's turn and waits for it to end, so it can be opened again.
    Close {
        thread_id: String,
    },
    StopAgent {
        thread_id: String,
    },
    Rename {
        thread_id: String,
        title: String,
    },
    /// Erases the chat, its agents' threads and its files folder for good.
    Delete {
        thread_id: String,
    },
    /// Answers a tool's request for a yes; `false` when it no longer waits.
    Approve {
        id: u64,
        decision: Decision,
    },
    /// The thread's own approval mode; `None` goes back to the default in settings.
    SetApprovals {
        thread_id: String,
        policy: Option<ApprovalPolicy>,
    },
    /// Answers with the thread's own approval mode, or none when it uses the default.
    GetApprovals {
        thread_id: String,
    },
}

/// What `/settings` changes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SettingsRequest {
    /// Answers with the input limit it gives.
    ContextPolicy(ContextPolicy),
    MaxAgentsAtWork(usize),
    /// Answers with the minutes saved, after the config's bounds.
    IdleScreenMinutes(u32),
    MaxGoalRounds(u32),
    MemoryChars(usize),
    /// Answers with the characters saved, after the config's bounds.
    WorkHistoryChars(usize),
    Switch {
        switch: Switch,
        on: bool,
    },
    /// The version control branch prefix; empty adds none.
    BranchPrefix(String),
    /// Answers with the mode switched to.
    NextMode,
    /// Switches to the mode of this name.
    Mode(String),
    /// Saves a provider; a model is chosen with `/model`.
    Connect {
        kind: ProviderKind,
        name: Option<String>,
        base_url: String,
        api_key: Option<Secret>,
    },
    /// Forgets a provider. Refused while its model is the chosen one.
    RemoveProvider {
        name: String,
    },
    PluginEnabled {
        name: String,
        enabled: bool,
    },
    /// Kept in memory for this run only, never in the config file.
    PluginSecret {
        plugin: String,
        key: String,
        value: Secret,
    },
}

/// What a switch in the settings turns on or off.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Switch {
    Agent(String),
    Tool(String),
    Skill(String),
    Mcp(String),
    Memory,
    WorkHistory,
    DevChannel,
    /// On sets the policy, off goes back to asking.
    Approvals(ApprovalPolicy),
    Logging,
    /// Whether clients notify at all.
    Notifications,
    /// Whether notifications ring the terminal bell.
    SoundNotifications,
}

/// The files the user edits through the settings, and the MCP servers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CatalogRequest {
    Skill {
        name: String,
    },
    SaveSkill {
        name: String,
        text: String,
    },
    Agent {
        name: String,
    },
    SaveAgent {
        name: String,
        text: String,
    },
    McpServers,
    McpServerText {
        name: String,
    },
    /// Answers with the name it was saved under, or none when the text removed it.
    SaveMcpServer {
        old: Option<String>,
        text: String,
    },
    RestartMcp {
        name: String,
    },
    /// Answers with the page that signs in.
    SignInMcp {
        name: String,
    },
    Snippets,
    /// Empty text removes the snippet.
    SaveSnippet {
        name: String,
        text: String,
    },
    Profile,
    SaveProfile(Profile),
}

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
    /// The files the chat's commands changed in its project.
    ChangedFiles {
        thread_id: String,
    },
}

/// Projects, and the git state and files of one. Git and files are read only inside
/// a saved project's folder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProjectRequest {
    List,
    /// Saves a new project, or changes the one with `id`; answers with the project.
    Save {
        id: Option<String>,
        name: String,
        description: String,
        dir: PathBuf,
    },
    Remove {
        id: String,
    },
    Git {
        id: String,
    },
    /// The changes against the last commit, file by file with their hunks; one file
    /// when `path` is given. Untracked files show as all added.
    Diff {
        id: String,
        path: Option<String>,
    },
    /// Refused while the work tree has changes, so nothing is lost.
    SwitchBranch {
        id: String,
        branch: String,
    },
    /// The entries of `path`, a folder relative to the project's, `""` for its top.
    Files {
        id: String,
        path: String,
    },
    /// The files under the workspace and the pictures folder, newest first.
    Outputs,
    /// The names of the project's environment variables. Values are never sent back.
    Env {
        id: String,
    },
    /// Sets one variable for the project's commands; answers with the names.
    SetEnv {
        id: String,
        name: String,
        value: Secret,
    },
    /// Removes one; answers with the names left.
    RemoveEnv {
        id: String,
        name: String,
    },
    /// A text file for a preview, read only under the workspace, the pictures folder or
    /// a saved project, and only up to `MAX_PREVIEW_BYTES`.
    ReadOutput {
        path: PathBuf,
    },
    /// Removes a file under the workspace or the pictures folder, never a project's.
    DeleteOutput {
        path: PathBuf,
    },
}

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Event {
    Thread {
        thread_id: String,
        event: Box<ThreadEvent>,
    },
    /// The thread's task ended; sending to it opens it again.
    Ended { thread_id: String },
}

/// Everything a client shows that changes without being asked: sent again whole
/// whenever any of it changes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Status {
    /// The model threads talk to, as `provider/model`.
    pub model: Option<String>,
    /// The core is still looking for a model over the network.
    pub discovering_model: bool,
    pub input_limit: u64,
    pub context_policy: Option<ContextPolicy>,
    pub effort: Option<Effort>,
    pub mode: String,
    pub approvals: ApprovalPolicy,
    pub max_goal_rounds: u32,
    pub idle_screen_minutes: u32,
    /// Whether clients notify at all.
    pub notifications: bool,
    pub sound_notifications: bool,
    pub mcp: Vec<McpStatus>,
    pub mcp_starting: bool,
    pub plugins: Vec<PluginInfo>,
    /// The newer release found on the update channel.
    pub available_update: Option<String>,
    /// How far an update install got, while one runs or once it is done.
    pub install: Option<InstallProgress>,
    /// Shipped files this start brought to the build's version.
    pub updated_builtin: Vec<String>,
    /// Where pictures of messages and tool results are kept.
    pub artifacts_dir: PathBuf,
}
