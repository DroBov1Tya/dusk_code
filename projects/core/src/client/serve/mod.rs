//! The core's side of the protocol: one task that owns the runtime, answers requests in
//! the order they come, forwards the events of open threads and keeps the status fresh.

mod admit;
mod catalog;
mod git;
#[cfg(test)]
mod plan_tests;
mod plugin;
mod projects;
mod publish;
mod pulls;
mod schedules;
mod settings;
mod status;
mod terminal;
#[cfg(test)]
mod tests;
mod thread;
mod user_runs;
mod voice;
mod work;

pub use status::blank_status;
use status::read_status;

use crate::{
    client::Scope,
    message::{commands, sources::HostFiles},
    runtime::{Runtime, ThreadInput},
    terminal::Terminals,
};
use anyhow::{Result, anyhow};
use dusk_protocol::client::{Event, ProjectRequest, Request, Response, Status, ThreadRequest};
use dusk_protocol::thread::Turn;
use dusk_protocol::tools::Question;
use parking_lot::Mutex;
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::Duration,
};
use tokio::{
    sync::{mpsc, oneshot, watch},
    task::JoinHandle,
};
use tracing::warn;

/// How long a closing thread may take to stop its turn and finish.
const CLOSE_TIMEOUT: Duration = Duration::from_secs(10);

/// How long a thread whose clients all vanished keeps running.
const ORPHAN_GRACE: Duration = Duration::from_secs(60);

/// How often the status is read again for what changes in the background, such as a
/// model found over the network or an MCP server that settled.
const STATUS_PERIOD: Duration = Duration::from_millis(100);
/// How often the timetable is read for tasks whose time has come.
const SCHEDULE_PERIOD: Duration = Duration::from_secs(30);

/// Who is attached to the core; ids are never reused within a run.
pub type ClientId = u64;

/// Where the events of open threads go: one queue for each attached client, and the scope
/// that says which threads' events it gets.
pub type Subscribers = Arc<Mutex<Vec<Subscriber>>>;

pub struct Subscriber {
    pub client: ClientId,
    pub scope: Scope,
    pub queue: mpsc::UnboundedSender<Event>,
}

pub enum Message {
    /// A client joined; `source` is the name its threads carry, `scope` what it may ask.
    Attach {
        client: ClientId,
        source: String,
        scope: Scope,
    },
    Call(Call),
    /// A client went away: it leaves every thread it had open.
    Detach(ClientId),
}

pub struct Call {
    pub client: ClientId,
    pub request: Request,
    pub reply: oneshot::Sender<Response>,
}

/// An open thread: where its input goes, the task that forwards its events to every
/// subscriber and the clients that keep it open.
struct OpenThread {
    input: ThreadInput,
    forward: JoinHandle<()>,
    clients: HashSet<ClientId>,
    /// When the last client that kept it open went away; a turn gets `ORPHAN_GRACE` to be
    /// picked up again, as after a reload, before it is stopped.
    orphaned: Option<std::time::Instant>,
    /// Started by a schedule with nobody watching: it runs to the end of its turn and
    /// then closes, rather than stopping when no client keeps it.
    unattended: bool,
    /// Set by the forwarding task once the thread has worked and is idle again.
    done: Arc<std::sync::atomic::AtomicBool>,
    /// The turn running now, as the forwarding task sees it; a client that joins
    /// mid-turn is told so before any later event.
    running: Arc<Mutex<Option<Running>>>,
}

/// A running turn as a client that joins is told of it.
#[derive(Clone)]
struct Running {
    turn: Turn,
    /// What the user is asked while the turn waits for a decision.
    question: Option<Question>,
}

/// An attached client as the core knows it.
struct Attached {
    /// The source of the threads it creates.
    source: String,
    scope: Scope,
}

pub struct Server {
    runtime: Arc<Runtime>,
    clients: HashMap<ClientId, Attached>,
    /// The client whose request is being answered.
    caller: ClientId,
    subscribers: Subscribers,
    status: Arc<watch::Sender<Status>>,
    threads: HashMap<String, OpenThread>,
    terminals: Terminals,
    /// The repository status of the projects clients watch.
    pulls: crate::project::forge::Tracker,
    /// Commands users run from code blocks, under way.
    user_runs: user_runs::UserRuns,
}

impl Server {
    pub fn new(runtime: Runtime, subscribers: Subscribers) -> (Self, watch::Receiver<Status>) {
        let status = read_status(&runtime, None);
        let (sender, receiver) = watch::channel(status);
        let server = Self {
            runtime: Arc::new(runtime),
            clients: HashMap::new(),
            caller: 0,
            subscribers,
            status: Arc::new(sender),
            threads: HashMap::new(),
            terminals: Terminals::new(crate::terminal::login_shell()),
            pulls: crate::project::forge::Tracker::default(),
            user_runs: user_runs::UserRuns::default(),
        };
        (server, receiver)
    }

    /// The name of the client being answered.
    pub(super) fn source(&self) -> &str {
        self.clients
            .get(&self.caller)
            .map_or("client", |attached| attached.source.as_str())
    }

    /// The chats the client being answered is bound to, or none when it sees them all.
    /// A caller that is not attached was refused before any handler.
    pub(super) fn bound(&self) -> Option<Arc<HashSet<String>>> {
        let attached = self.clients.get(&self.caller)?;
        attached.scope.bound().cloned()
    }

    /// Whether a message of the client being answered may name files of this machine.
    pub(super) fn host_files(&self) -> HostFiles {
        self.clients
            .get(&self.caller)
            .map_or(HostFiles::Left, |attached| attached.scope.host_files())
    }

    /// Answers calls until the client asks to shut down or goes away, then closes every
    /// thread and stops the runtime.
    pub async fn run(mut self, mut calls: mpsc::UnboundedReceiver<Message>) {
        let mut clock = tokio::time::interval(STATUS_PERIOD);
        clock.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        // The first tick is at once, so runs missed while the app was closed start now.
        let mut timetable = tokio::time::interval(SCHEDULE_PERIOD);
        timetable.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let mut repositories = tokio::time::interval(crate::project::forge::TRACK_PERIOD);
        repositories.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let last = loop {
            tokio::select! {
                call = calls.recv() => match call {
                    None => break None,
                    Some(Message::Attach { client, source, scope }) => {
                        self.clients.insert(client, Attached { source, scope });
                    }
                    Some(Message::Detach(client)) => self.detach(client).await,
                    Some(Message::Call(Call { client, request, reply })) => {
                        // Every request passes here first, whichever part answers it.
                        let listed = match self.admit(client, &request).await {
                            Ok(listed) => listed,
                            Err(refusal) => {
                                let _ = reply.send(Response::Error(refusal.to_owned()));
                                continue;
                            }
                        };
                        match request {
                            Request::Shutdown => break Some(reply),
                            Request::InstallUpdate { tag } => self.install_update(tag, reply),
                            Request::Store(request) => self.store(request, reply),
                            Request::Project(ProjectRequest::Repository { id, request }) => {
                                self.repository(id, request, reply)
                            }
                            Request::Project(ProjectRequest::RepositoryStatus { id }) => {
                                self.repository_status(client, id, reply)
                            }
                            Request::Project(ProjectRequest::PullStatus { id, number }) => {
                                self.pull_status(id, number, reply)
                            }
                            Request::Thread(ThreadRequest::RunCommand { thread_id, command, run }) => {
                                self.run_command(client, thread_id, command, run, reply)
                            }
                            Request::Thread(ThreadRequest::StopRun { thread_id, run }) => {
                                let response = match self.stop_run(client, thread_id, run) {
                                    true => Response::Done,
                                    false => Response::Error("the command is not running".into()),
                                };
                                let _ = reply.send(response);
                            }
                            request => {
                                self.caller = client;
                                let mut response = match self.handle(request).await {
                                    Ok(response) => response,
                                    Err(error) => Response::Error(format!("{error:#}")),
                                };
                                if let Some(ids) = listed {
                                    response = self.cut(&ids, response).await;
                                }
                                // The client reads the status once it has the answer.
                                self.refresh_status();
                                let _ = reply.send(response);
                            }
                        }
                    }
                },
                _ = clock.tick() => {
                    self.refresh_status();
                    self.close_orphans().await;
                }
                _ = timetable.tick() => self.run_due_schedules().await,
                _ = repositories.tick() => self.track_repositories(),
            }
        };
        self.terminals.close_all();
        let threads: Vec<String> = self.threads.keys().cloned().collect();
        for thread_id in threads {
            self.close(&thread_id).await;
        }
        match Arc::into_inner(self.runtime) {
            Some(runtime) => runtime.shutdown().await,
            None => warn!("an update install still holds the runtime; it stops without a shutdown"),
        }
        if let Some(reply) = last {
            let _ = reply.send(Response::Done);
        }
    }

    pub(super) async fn handle(&mut self, request: Request) -> Result<Response> {
        match request {
            Request::Thread(request) => self.thread(request).await,
            Request::Settings(request) => self.settings(request).await,
            Request::Catalog(request) => self.catalog(request).await,
            Request::Work(request) => self.work(request).await,
            Request::Project(request) if schedules::is_schedule(&request) => {
                self.schedule(request).await
            }
            Request::Project(request) => self.project(request).await,
            Request::Command(command) => Ok(Response::Answer(Box::new(
                commands::answer(command, &self.runtime).await?,
            ))),
            Request::Voice(request) => self.voice(request).await,
            Request::Terminal(request) => self.terminal(request).await,
            Request::SystemReport => Ok(Response::System(self.runtime.system_report().await?)),
            Request::Plugin(request) => self.plugin(request).await,
            Request::InstallUpdate { .. } | Request::Store(_) | Request::Shutdown => {
                Err(anyhow!("this request is answered by the loop"))
            }
        }
    }
}
