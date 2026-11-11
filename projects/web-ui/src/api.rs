//! The core's requests as typed calls, so a view says what it wants and not which
//! response variant carries it. Every error is the core's own text.

use crate::link::request;
use dusk_protocol::{
    catalog::NodeText,
    client::{
        Answer, CatalogRequest, Command, ProjectRequest, Request, Response, Secret,
        SettingsRequest, Switch, ThreadRequest, VoiceRequest, WorkRequest,
    },
    config::{
        ApprovalPolicy, McpInfo, Profile, SecretsInfo, Settings, Snippet, VoiceModels,
        VoiceSettings,
    },
    model::ProviderStatus,
    project::{Change, FileDiff, FileEntry, GitInfo, OutputFile, Project},
    system::SystemReport,
    thread::{ContextReport, JobInfo, Message, Task, Thread},
    tools::Decision,
};

pub type Res<T> = Result<T, String>;

fn unexpected<T>(response: Response) -> Res<T> {
    Err(format!("The core gave an unexpected answer: {response:?}"))
}

/// A request whose answer is `Done`.
pub async fn done(call: Request) -> Res<()> {
    match request(call).await? {
        Response::Done => Ok(()),
        other => unexpected(other),
    }
}

macro_rules! ask {
    ($call:expr, $variant:pat => $value:expr) => {
        match request($call).await? {
            $variant => Ok($value),
            other => unexpected(other),
        }
    };
}

fn thread(call: ThreadRequest) -> Request {
    Request::Thread(call)
}

fn work(call: WorkRequest) -> Request {
    Request::Work(call)
}

fn project(call: ProjectRequest) -> Request {
    Request::Project(call)
}

fn catalog(call: CatalogRequest) -> Request {
    Request::Catalog(call)
}

fn settings(call: SettingsRequest) -> Request {
    Request::Settings(call)
}

// Threads

pub async fn threads() -> Res<Vec<Thread>> {
    ask!(thread(ThreadRequest::List), Response::Threads(t) => t)
}

pub async fn new_thread(cwd: String) -> Res<String> {
    let cwd = cwd.into();
    ask!(thread(ThreadRequest::New { cwd }), Response::Opened { thread_id } => thread_id)
}

pub async fn resume(thread_id: &str, cwd: String) -> Res<String> {
    let (thread_id, cwd) = (thread_id.to_owned(), cwd.into());
    ask!(thread(ThreadRequest::Resume { thread_id, cwd }), Response::Opened { thread_id } => thread_id)
}

pub async fn fork(summary: String, cwd: String) -> Res<String> {
    let cwd = cwd.into();
    ask!(thread(ThreadRequest::Fork { summary, cwd }), Response::Opened { thread_id } => thread_id)
}

pub async fn messages(thread_id: &str) -> Res<Vec<Message>> {
    let thread_id = thread_id.to_owned();
    ask!(thread(ThreadRequest::Messages { thread_id }), Response::Messages(m) => m)
}

pub async fn tasks(thread_id: &str) -> Res<Vec<Task>> {
    let thread_id = thread_id.to_owned();
    ask!(thread(ThreadRequest::Tasks { thread_id }), Response::Tasks(t) => t)
}

pub async fn send(thread_id: &str, text: String, now: bool) -> Res<()> {
    let thread_id = thread_id.to_owned();
    done(thread(ThreadRequest::Send {
        thread_id,
        text,
        now,
    }))
    .await
}

pub async fn cancel(thread_id: &str) -> Res<()> {
    done(thread(ThreadRequest::Cancel {
        thread_id: thread_id.to_owned(),
    }))
    .await
}

pub async fn close(thread_id: &str) -> Res<()> {
    done(thread(ThreadRequest::Close {
        thread_id: thread_id.to_owned(),
    }))
    .await
}

pub async fn stop_agent(thread_id: &str) -> Res<()> {
    done(thread(ThreadRequest::StopAgent {
        thread_id: thread_id.to_owned(),
    }))
    .await
}

pub async fn rename(thread_id: &str, title: String) -> Res<()> {
    done(thread(ThreadRequest::Rename {
        thread_id: thread_id.to_owned(),
        title,
    }))
    .await
}

pub async fn delete_thread(thread_id: &str) -> Res<()> {
    done(thread(ThreadRequest::Delete {
        thread_id: thread_id.to_owned(),
    }))
    .await
}

pub async fn approve(id: u64, decision: Decision) -> Res<bool> {
    ask!(thread(ThreadRequest::Approve { id, decision }), Response::Waiting(w) => w)
}

pub async fn set_approvals(thread_id: &str, policy: Option<ApprovalPolicy>) -> Res<()> {
    done(thread(ThreadRequest::SetApprovals {
        thread_id: thread_id.to_owned(),
        policy,
    }))
    .await
}

pub async fn get_approvals(thread_id: &str) -> Res<Option<ApprovalPolicy>> {
    let thread_id = thread_id.to_owned();
    ask!(thread(ThreadRequest::GetApprovals { thread_id }), Response::Approvals(a) => a)
}

pub async fn run_command(thread_id: &str, command: String) -> Res<String> {
    let thread_id = thread_id.to_owned();
    ask!(thread(ThreadRequest::RunCommand { thread_id, command }), Response::Text(t) => t)
}

// Work of a thread

pub async fn jobs(thread_id: &str) -> Res<Vec<JobInfo>> {
    let thread_id = thread_id.to_owned();
    ask!(work(WorkRequest::Jobs { thread_id }), Response::Jobs(j) => j)
}

pub async fn job_output(thread_id: &str, job_id: &str) -> Res<String> {
    let (thread_id, job_id) = (thread_id.to_owned(), job_id.to_owned());
    ask!(work(WorkRequest::JobOutput { thread_id, job_id }), Response::Text(t) => t)
}

pub async fn stop_job(thread_id: &str, job_id: &str) -> Res<String> {
    let (thread_id, job_id) = (thread_id.to_owned(), job_id.to_owned());
    ask!(work(WorkRequest::StopJob { thread_id, job_id }), Response::Text(t) => t)
}

pub async fn plan(thread_id: &str) -> Res<Option<String>> {
    let thread_id = thread_id.to_owned();
    ask!(work(WorkRequest::Plan { thread_id }), Response::Plan(p) => p)
}

pub async fn save_plan(thread_id: &str, text: String) -> Res<()> {
    let thread_id = thread_id.to_owned();
    done(work(WorkRequest::SavePlan { thread_id, text })).await
}

pub async fn chat_diff(thread_id: &str, path: Option<String>) -> Res<Vec<FileDiff>> {
    let thread_id = thread_id.to_owned();
    ask!(work(WorkRequest::Diff { thread_id, path }), Response::Diff(d) => d)
}

pub async fn changed_files(thread_id: &str) -> Res<Vec<Change>> {
    let thread_id = thread_id.to_owned();
    ask!(work(WorkRequest::ChangedFiles { thread_id }), Response::Changes(c) => c)
}

// Projects

pub async fn projects() -> Res<Vec<Project>> {
    ask!(project(ProjectRequest::List), Response::Projects(p) => p)
}

pub async fn save_project(
    id: Option<String>,
    name: String,
    description: String,
    dir: String,
) -> Res<Project> {
    let dir = dir.into();
    ask!(
        project(ProjectRequest::Save { id, name, description, dir }),
        Response::Project(p) => p
    )
}

pub async fn remove_project(id: &str) -> Res<()> {
    done(project(ProjectRequest::Remove { id: id.to_owned() })).await
}

pub async fn git(id: &str) -> Res<GitInfo> {
    ask!(project(ProjectRequest::Git { id: id.to_owned() }), Response::Git(g) => g)
}

pub async fn project_diff(id: &str, path: Option<String>) -> Res<Vec<FileDiff>> {
    let id = id.to_owned();
    ask!(project(ProjectRequest::Diff { id, path }), Response::Diff(d) => d)
}

pub async fn switch_branch(id: &str, branch: String) -> Res<()> {
    done(project(ProjectRequest::SwitchBranch {
        id: id.to_owned(),
        branch,
    }))
    .await
}

pub async fn files(id: &str, path: String) -> Res<Vec<FileEntry>> {
    let id = id.to_owned();
    ask!(project(ProjectRequest::Files { id, path }), Response::Files(f) => f)
}

pub async fn outputs() -> Res<Vec<OutputFile>> {
    ask!(project(ProjectRequest::Outputs), Response::Outputs(o) => o)
}

pub async fn read_output(path: String) -> Res<String> {
    let path = path.into();
    ask!(project(ProjectRequest::ReadOutput { path }), Response::Text(t) => t)
}

pub async fn delete_output(path: String) -> Res<()> {
    done(project(ProjectRequest::DeleteOutput { path: path.into() })).await
}

pub async fn env_names(id: &str) -> Res<Vec<String>> {
    ask!(project(ProjectRequest::Env { id: id.to_owned() }), Response::Names(n) => n)
}

pub async fn set_env(id: &str, name: String, value: String) -> Res<Vec<String>> {
    let id = id.to_owned();
    let value = Secret(value);
    ask!(project(ProjectRequest::SetEnv { id, name, value }), Response::Names(n) => n)
}

pub async fn remove_env(id: &str, name: String) -> Res<Vec<String>> {
    let id = id.to_owned();
    ask!(project(ProjectRequest::RemoveEnv { id, name }), Response::Names(n) => n)
}

// Commands and reports

pub async fn command(command: Command) -> Res<Answer> {
    ask!(Request::Command(command), Response::Answer(a) => *a)
}

pub async fn settings_panel() -> Res<Settings> {
    match command(Command::Settings).await? {
        Answer::Settings(panel) => Ok(*panel),
        other => Err(format!("The core answered {other:?}, not the settings.")),
    }
}

pub async fn context_report(thread_id: Option<String>) -> Res<ContextReport> {
    match command(Command::Context { thread_id }).await? {
        Answer::Context(report) => Ok(report),
        other => Err(format!("The core answered {other:?}, not a report.")),
    }
}

pub async fn system_report() -> Res<SystemReport> {
    ask!(Request::SystemReport, Response::System(r) => r)
}

pub async fn install_update(tag: String) -> Res<String> {
    ask!(Request::InstallUpdate { tag }, Response::Path(p) => p.display().to_string())
}

// Settings

pub async fn set(call: SettingsRequest) -> Res<()> {
    done(settings(call)).await
}

pub async fn switch(switch: Switch, on: bool) -> Res<()> {
    set(SettingsRequest::Switch { switch, on }).await
}

pub async fn next_mode() -> Res<String> {
    ask!(settings(SettingsRequest::NextMode), Response::Text(t) => t)
}

pub async fn number(call: SettingsRequest) -> Res<u64> {
    ask!(settings(call), Response::Number(n) => n)
}

pub async fn check_provider(call: SettingsRequest) -> Res<ProviderStatus> {
    ask!(settings(call), Response::Provider(p) => p)
}

pub async fn connect_provider(call: SettingsRequest) -> Res<String> {
    ask!(settings(call), Response::Text(t) => t)
}

pub async fn set_secret(name: String, value: String) -> Res<()> {
    let value = Secret(value);
    set(SettingsRequest::SetSecret { name, value }).await
}

// Catalog

pub async fn skill(name: &str) -> Res<NodeText> {
    let name = name.to_owned();
    ask!(catalog(CatalogRequest::Skill { name }), Response::Node(n) => n)
}

pub async fn save_skill(name: &str, text: String) -> Res<NodeText> {
    let name = name.to_owned();
    ask!(catalog(CatalogRequest::SaveSkill { name, text }), Response::Node(n) => n)
}

pub async fn agent(name: &str) -> Res<NodeText> {
    let name = name.to_owned();
    ask!(catalog(CatalogRequest::Agent { name }), Response::Node(n) => n)
}

pub async fn save_agent(name: &str, text: String) -> Res<NodeText> {
    let name = name.to_owned();
    ask!(catalog(CatalogRequest::SaveAgent { name, text }), Response::Node(n) => n)
}

pub async fn mcp_servers() -> Res<Vec<McpInfo>> {
    ask!(catalog(CatalogRequest::McpServers), Response::Mcp(m) => m)
}

pub async fn mcp_server_text(name: &str) -> Res<String> {
    let name = name.to_owned();
    ask!(catalog(CatalogRequest::McpServerText { name }), Response::Text(t) => t)
}

pub async fn save_mcp_server(old: Option<String>, text: String) -> Res<Option<String>> {
    ask!(catalog(CatalogRequest::SaveMcpServer { old, text }), Response::Name(n) => n)
}

pub async fn restart_mcp(name: &str) -> Res<()> {
    done(catalog(CatalogRequest::RestartMcp {
        name: name.to_owned(),
    }))
    .await
}

pub async fn sign_in_mcp(name: &str) -> Res<String> {
    let name = name.to_owned();
    ask!(catalog(CatalogRequest::SignInMcp { name }), Response::Text(t) => t)
}

pub async fn snippets() -> Res<Vec<Snippet>> {
    ask!(catalog(CatalogRequest::Snippets), Response::Snippets(s) => s)
}

pub async fn save_snippet(name: String, text: String) -> Res<()> {
    done(catalog(CatalogRequest::SaveSnippet { name, text })).await
}

pub async fn profile() -> Res<Profile> {
    ask!(catalog(CatalogRequest::Profile), Response::Profile(p) => p)
}

pub async fn save_profile(profile: Profile) -> Res<()> {
    done(catalog(CatalogRequest::SaveProfile(profile))).await
}

pub async fn secrets() -> Res<SecretsInfo> {
    ask!(catalog(CatalogRequest::Secrets), Response::Secrets(s) => s)
}

pub async fn voice_settings() -> Res<VoiceSettings> {
    ask!(catalog(CatalogRequest::Voice), Response::Voice(v) => v)
}

// Voice

pub async fn voice_models() -> Res<VoiceModels> {
    ask!(Request::Voice(VoiceRequest::Models), Response::VoiceModels(m) => m)
}

pub async fn voice(call: VoiceRequest) -> Res<()> {
    done(Request::Voice(call)).await
}

pub async fn transcribe(audio: Vec<u8>) -> Res<String> {
    ask!(Request::Voice(VoiceRequest::Transcribe { audio }), Response::Text(t) => t)
}

pub async fn speak(text: String) -> Res<dusk_protocol::tools::Card> {
    ask!(Request::Voice(VoiceRequest::Speak { text }), Response::Card(c) => c)
}
