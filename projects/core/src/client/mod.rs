//! How a client talks to the core: `serve` starts the core's side and hands back a
//! `Client`. Every call goes through the protocol's requests, so a client in this
//! process and one in another see the same core.
//!
//! The calls are grouped as the protocol groups them: `client.threads()`,
//! `client.settings()`, `client.catalog()`, `client.work()`, `client.projects()`, and the rest on `Client`.
//! Events arrive on `client.events`, the status on `client.status`.

mod serve;

use std::{future::Future, path::PathBuf};

use anyhow::{Result, anyhow, bail, ensure};
use dusk_protocol::{
    catalog::NodeText,
    client::{
        CatalogRequest, Event, Hello, PROTOCOL_VERSION, ProjectRequest, Request, Response, Secret,
        SettingsRequest, Status, Switch, ThreadRequest, WorkRequest,
    },
    command::{Answer, Command},
    config::{ContextPolicy, ProviderKind},
    conversation::{LocalUnsent, Message},
    mcp::McpInfo,
    project::{FileEntry, GitInfo, Project},
    system::SystemReport,
    thread::Thread,
    tools::Decision,
    work::{JobInfo, Task},
};
use tokio::sync::{mpsc, oneshot, watch};

use crate::runtime::Runtime;
use serve::{Call, Server};

/// Starts the core's side of the protocol over `runtime` for the client that said
/// `hello`.
pub fn serve(runtime: Runtime, hello: Hello) -> Result<Client> {
    ensure!(
        hello.protocol == PROTOCOL_VERSION,
        "the client speaks protocol {}, the core {PROTOCOL_VERSION}",
        hello.protocol
    );
    let (calls, inbox) = mpsc::unbounded_channel();
    let (events_sender, events) = mpsc::unbounded_channel();
    let (server, status) = Server::new(runtime, hello.client, events_sender);
    tokio::spawn(server.run(inbox));
    Ok(Client {
        events,
        status,
        calls,
    })
}

pub struct Client {
    /// The events of the open threads, in the order they happened.
    pub events: mpsc::UnboundedReceiver<Event>,
    /// The latest status; `changed` wakes when the core sends a new one.
    pub status: watch::Receiver<Status>,
    calls: mpsc::UnboundedSender<Call>,
}

impl Client {
    pub fn threads(&self) -> Threads<'_> {
        Threads(self)
    }

    pub fn settings(&self) -> Settings<'_> {
        Settings(self)
    }

    pub fn catalog(&self) -> Catalog<'_> {
        Catalog(self)
    }

    pub fn work(&self) -> Work<'_> {
        Work(self)
    }

    pub fn projects(&self) -> Projects<'_> {
        Projects(self)
    }

    /// A slash command that does not run in a thread.
    pub async fn command(&self, command: Command) -> Result<Answer> {
        match self.call(Request::Command(command)).await? {
            Response::Answer(answer) => Ok(*answer),
            other => Err(unexpected(other)),
        }
    }

    pub async fn system_report(&self) -> Result<SystemReport> {
        match self.call(Request::SystemReport).await? {
            Response::System(report) => Ok(report),
            other => Err(unexpected(other)),
        }
    }

    /// Installs release `tag`. The future lives on its own, so a client can wait for
    /// it beside its other work; the status shows the progress meanwhile.
    pub fn install_update(&self, tag: &str) -> impl Future<Output = Result<PathBuf>> + use<> {
        let reply = self.send(Request::InstallUpdate {
            tag: tag.to_owned(),
        });
        async move {
            match answer(reply.await)? {
                Response::Path(path) => Ok(path),
                other => Err(unexpected(other)),
            }
        }
    }

    /// Closes every thread and stops the core; returns once it has stopped. Every call
    /// after it fails.
    pub async fn shutdown(&self) {
        let _ = self.send(Request::Shutdown).await;
    }

    async fn call(&self, request: Request) -> Result<Response> {
        answer(self.send(request).await)
    }

    async fn done(&self, request: Request) -> Result<()> {
        match self.call(request).await? {
            Response::Done => Ok(()),
            other => Err(unexpected(other)),
        }
    }

    async fn text(&self, request: Request) -> Result<String> {
        match self.call(request).await? {
            Response::Text(text) => Ok(text),
            other => Err(unexpected(other)),
        }
    }

    async fn node(&self, request: Request) -> Result<NodeText> {
        match self.call(request).await? {
            Response::Node(node) => Ok(node),
            other => Err(unexpected(other)),
        }
    }

    fn send(&self, request: Request) -> oneshot::Receiver<Response> {
        let (reply, answer) = oneshot::channel();
        // A stopped core drops the call, and the receiver says so.
        let _ = self.calls.send(Call { request, reply });
        answer
    }
}

/// The calls on threads; events of an open thread arrive on `Client::events`.
pub struct Threads<'a>(&'a Client);

impl Threads<'_> {
    /// A new thread in `cwd`; it is created now and opened for messages.
    pub async fn start(&self, cwd: PathBuf) -> Result<String> {
        self.opened(ThreadRequest::New { cwd }).await
    }

    pub async fn resume(&self, thread_id: &str, cwd: PathBuf) -> Result<String> {
        let thread_id = thread_id.to_owned();
        self.opened(ThreadRequest::Resume { thread_id, cwd }).await
    }

    pub async fn fork(&self, summary: &str, cwd: PathBuf) -> Result<String> {
        let summary = summary.to_owned();
        self.opened(ThreadRequest::Fork { summary, cwd }).await
    }

    pub async fn list(&self) -> Result<Vec<Thread>> {
        match self.call(ThreadRequest::List).await? {
            Response::Threads(threads) => Ok(threads),
            other => Err(unexpected(other)),
        }
    }

    pub async fn messages(&self, thread_id: &str) -> Result<Vec<Message>> {
        let thread_id = thread_id.to_owned();
        match self.call(ThreadRequest::Messages { thread_id }).await? {
            Response::Messages(messages) => Ok(messages),
            other => Err(unexpected(other)),
        }
    }

    pub async fn tasks(&self, thread_id: &str) -> Result<Vec<Task>> {
        let thread_id = thread_id.to_owned();
        match self.call(ThreadRequest::Tasks { thread_id }).await? {
            Response::Tasks(tasks) => Ok(tasks),
            other => Err(unexpected(other)),
        }
    }

    pub async fn local_unsent(&self, thread_id: &str) -> Result<Vec<LocalUnsent>> {
        let thread_id = thread_id.to_owned();
        match self.call(ThreadRequest::LocalUnsent { thread_id }).await? {
            Response::LocalUnsent(messages) => Ok(messages),
            other => Err(unexpected(other)),
        }
    }

    pub async fn resend_local_unsent(&self, thread_id: &str, id: &str) -> Result<()> {
        let (thread_id, id) = (thread_id.to_owned(), id.to_owned());
        self.done(ThreadRequest::ResendLocalUnsent { thread_id, id })
            .await
    }

    /// Queues `text`, or puts it before what is queued when `now`.
    pub async fn send(&self, thread_id: &str, text: String, now: bool) -> Result<()> {
        let thread_id = thread_id.to_owned();
        self.done(ThreadRequest::Send {
            thread_id,
            text,
            now,
        })
        .await
    }

    pub async fn cancel(&self, thread_id: &str) -> Result<()> {
        let thread_id = thread_id.to_owned();
        self.done(ThreadRequest::Cancel { thread_id }).await
    }

    /// Stops the thread's turn and waits for its task to end.
    pub async fn close(&self, thread_id: &str) -> Result<()> {
        let thread_id = thread_id.to_owned();
        self.done(ThreadRequest::Close { thread_id }).await
    }

    pub async fn stop_agent(&self, thread_id: &str) -> Result<()> {
        let thread_id = thread_id.to_owned();
        self.done(ThreadRequest::StopAgent { thread_id }).await
    }

    /// Answers a tool's request; `false` when it no longer waits.
    pub async fn approve(&self, id: u64, decision: Decision) -> Result<bool> {
        match self.call(ThreadRequest::Approve { id, decision }).await? {
            Response::Waiting(waiting) => Ok(waiting),
            other => Err(unexpected(other)),
        }
    }

    async fn opened(&self, request: ThreadRequest) -> Result<String> {
        match self.call(request).await? {
            Response::Opened { thread_id } => Ok(thread_id),
            other => Err(unexpected(other)),
        }
    }

    async fn call(&self, request: ThreadRequest) -> Result<Response> {
        self.0.call(Request::Thread(request)).await
    }

    async fn done(&self, request: ThreadRequest) -> Result<()> {
        self.0.done(Request::Thread(request)).await
    }
}

/// The calls that change `/settings`.
pub struct Settings<'a>(&'a Client);

impl Settings<'_> {
    /// Returns the input limit the policy gives.
    pub async fn context_policy(&self, policy: ContextPolicy) -> Result<u64> {
        self.number(SettingsRequest::ContextPolicy(policy)).await
    }

    pub async fn max_agents_at_work(&self, limit: usize) -> Result<()> {
        self.done(SettingsRequest::MaxAgentsAtWork(limit)).await
    }

    /// Returns the minutes saved, after the config's bounds.
    pub async fn idle_screen_minutes(&self, minutes: u32) -> Result<u64> {
        self.number(SettingsRequest::IdleScreenMinutes(minutes))
            .await
    }

    pub async fn max_goal_rounds(&self, rounds: u32) -> Result<()> {
        self.done(SettingsRequest::MaxGoalRounds(rounds)).await
    }

    pub async fn memory_chars(&self, chars: usize) -> Result<()> {
        self.done(SettingsRequest::MemoryChars(chars)).await
    }

    /// Returns the characters saved, after the config's bounds.
    pub async fn work_history_chars(&self, chars: usize) -> Result<u64> {
        self.number(SettingsRequest::WorkHistoryChars(chars)).await
    }

    pub async fn switch(&self, switch: Switch, on: bool) -> Result<()> {
        self.done(SettingsRequest::Switch { switch, on }).await
    }

    /// Returns the mode switched to.
    pub async fn next_mode(&self) -> Result<String> {
        self.0
            .text(Request::Settings(SettingsRequest::NextMode))
            .await
    }

    pub async fn mode(&self, name: &str) -> Result<()> {
        self.done(SettingsRequest::Mode(name.to_owned())).await
    }

    /// Saves a provider and returns the name it was saved under.
    pub async fn connect(
        &self,
        kind: ProviderKind,
        name: Option<String>,
        base_url: String,
        api_key: Option<String>,
    ) -> Result<String> {
        let api_key = api_key.map(Secret);
        let request = SettingsRequest::Connect {
            kind,
            name,
            base_url,
            api_key,
        };
        self.0.text(Request::Settings(request)).await
    }

    pub async fn plugin_enabled(&self, name: &str, enabled: bool) -> Result<()> {
        let name = name.to_owned();
        self.done(SettingsRequest::PluginEnabled { name, enabled })
            .await
    }

    /// Kept for this run only, never written to the config file.
    pub async fn plugin_secret(&self, plugin: &str, key: &str, value: &str) -> Result<()> {
        let (plugin, key) = (plugin.to_owned(), key.to_owned());
        let value = Secret(value.to_owned());
        self.done(SettingsRequest::PluginSecret { plugin, key, value })
            .await
    }

    async fn number(&self, request: SettingsRequest) -> Result<u64> {
        match self.0.call(Request::Settings(request)).await? {
            Response::Number(number) => Ok(number),
            other => Err(unexpected(other)),
        }
    }

    async fn done(&self, request: SettingsRequest) -> Result<()> {
        self.0.done(Request::Settings(request)).await
    }
}

/// The files the user edits through the settings, and the MCP servers.
pub struct Catalog<'a>(&'a Client);

impl Catalog<'_> {
    pub async fn skill(&self, name: &str) -> Result<NodeText> {
        let name = name.to_owned();
        self.node(CatalogRequest::Skill { name }).await
    }

    pub async fn save_skill(&self, name: &str, text: &str) -> Result<NodeText> {
        let (name, text) = (name.to_owned(), text.to_owned());
        self.node(CatalogRequest::SaveSkill { name, text }).await
    }

    pub async fn agent(&self, name: &str) -> Result<NodeText> {
        let name = name.to_owned();
        self.node(CatalogRequest::Agent { name }).await
    }

    pub async fn save_agent(&self, name: &str, text: &str) -> Result<NodeText> {
        let (name, text) = (name.to_owned(), text.to_owned());
        self.node(CatalogRequest::SaveAgent { name, text }).await
    }

    pub async fn mcp_servers(&self) -> Result<Vec<McpInfo>> {
        match self.call(CatalogRequest::McpServers).await? {
            Response::Mcp(servers) => Ok(servers),
            other => Err(unexpected(other)),
        }
    }

    pub async fn mcp_server_text(&self, name: &str) -> Result<String> {
        let name = name.to_owned();
        let request = Request::Catalog(CatalogRequest::McpServerText { name });
        self.0.text(request).await
    }

    /// Returns the name it was saved under, or none when the text removed it.
    pub async fn save_mcp_server(&self, old: Option<&str>, text: &str) -> Result<Option<String>> {
        let (old, text) = (old.map(str::to_owned), text.to_owned());
        match self
            .call(CatalogRequest::SaveMcpServer { old, text })
            .await?
        {
            Response::Name(name) => Ok(name),
            other => Err(unexpected(other)),
        }
    }

    pub async fn restart_mcp(&self, name: &str) -> Result<()> {
        let name = name.to_owned();
        self.0
            .done(Request::Catalog(CatalogRequest::RestartMcp { name }))
            .await
    }

    /// Returns the page that signs in.
    pub async fn sign_in_mcp(&self, name: &str) -> Result<String> {
        let name = name.to_owned();
        let request = Request::Catalog(CatalogRequest::SignInMcp { name });
        self.0.text(request).await
    }

    async fn call(&self, request: CatalogRequest) -> Result<Response> {
        self.0.call(Request::Catalog(request)).await
    }

    async fn node(&self, request: CatalogRequest) -> Result<NodeText> {
        self.0.node(Request::Catalog(request)).await
    }
}

/// A thread's background jobs and sources.
pub struct Work<'a>(&'a Client);

impl Work<'_> {
    pub async fn jobs(&self, thread_id: &str) -> Result<Vec<JobInfo>> {
        let thread_id = thread_id.to_owned();
        match self
            .0
            .call(Request::Work(WorkRequest::Jobs { thread_id }))
            .await?
        {
            Response::Jobs(jobs) => Ok(jobs),
            other => Err(unexpected(other)),
        }
    }

    pub async fn job_output(&self, thread_id: &str, job_id: &str) -> Result<String> {
        let (thread_id, job_id) = (thread_id.to_owned(), job_id.to_owned());
        let request = Request::Work(WorkRequest::JobOutput { thread_id, job_id });
        self.0.text(request).await
    }

    pub async fn stop_job(&self, thread_id: &str, job_id: &str) -> Result<String> {
        let (thread_id, job_id) = (thread_id.to_owned(), job_id.to_owned());
        let request = Request::Work(WorkRequest::StopJob { thread_id, job_id });
        self.0.text(request).await
    }

    pub async fn pin_source(&self, thread_id: &str, id: &str, pinned: bool) -> Result<()> {
        let (thread_id, id) = (thread_id.to_owned(), id.to_owned());
        let request = WorkRequest::PinSource {
            thread_id,
            id,
            pinned,
        };
        self.0.done(Request::Work(request)).await
    }

    pub async fn remove_source(&self, thread_id: &str, id: &str) -> Result<()> {
        let (thread_id, id) = (thread_id.to_owned(), id.to_owned());
        let request = WorkRequest::RemoveSource { thread_id, id };
        self.0.done(Request::Work(request)).await
    }
}

pub struct Projects<'a>(&'a Client);

impl Projects<'_> {
    pub async fn list(&self) -> Result<Vec<Project>> {
        match self.0.call(Request::Project(ProjectRequest::List)).await? {
            Response::Projects(projects) => Ok(projects),
            other => Err(unexpected(other)),
        }
    }

    /// Saves a new project when `id` is `None`, or changes that one.
    pub async fn save(
        &self,
        id: Option<String>,
        name: String,
        description: String,
        dir: PathBuf,
    ) -> Result<Project> {
        let request = ProjectRequest::Save {
            id,
            name,
            description,
            dir,
        };
        match self.0.call(Request::Project(request)).await? {
            Response::Project(project) => Ok(project),
            other => Err(unexpected(other)),
        }
    }

    pub async fn remove(&self, id: &str) -> Result<()> {
        let id = id.to_owned();
        self.0.done(Request::Project(ProjectRequest::Remove { id })).await
    }

    pub async fn git(&self, id: &str) -> Result<GitInfo> {
        let id = id.to_owned();
        match self.0.call(Request::Project(ProjectRequest::Git { id })).await? {
            Response::Git(info) => Ok(info),
            other => Err(unexpected(other)),
        }
    }

    pub async fn switch_branch(&self, id: &str, branch: &str) -> Result<()> {
        let (id, branch) = (id.to_owned(), branch.to_owned());
        let request = ProjectRequest::SwitchBranch { id, branch };
        self.0.done(Request::Project(request)).await
    }

    /// The entries of `path`, a folder relative to the project's, `""` for its top.
    pub async fn files(&self, id: &str, path: &str) -> Result<Vec<FileEntry>> {
        let (id, path) = (id.to_owned(), path.to_owned());
        let request = ProjectRequest::Files { id, path };
        match self.0.call(Request::Project(request)).await? {
            Response::Files(files) => Ok(files),
            other => Err(unexpected(other)),
        }
    }
}

/// The response, or the error the core gave; a dropped reply means the core stopped.
fn answer(reply: Result<Response, oneshot::error::RecvError>) -> Result<Response> {
    match reply {
        Ok(Response::Error(error)) => Err(anyhow!(error)),
        Ok(response) => Ok(response),
        Err(_) => bail!("the core has stopped"),
    }
}

fn unexpected(response: Response) -> anyhow::Error {
    anyhow!("the core gave an unexpected answer: {response:?}")
}
