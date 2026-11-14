//! The core's side of the protocol: one task that owns the runtime, answers requests in
//! the order they come, forwards the events of open threads and keeps the status fresh.

mod catalog;
mod plugin;
mod projects;
mod schedules;
mod settings;
mod simulator;
mod status;
mod terminal;
mod thread;
mod voice;
mod work;

use status::read_status;

use crate::{
    message::commands,
    runtime::{Runtime, ThreadInput},
    terminal::Terminals,
};
use anyhow::{Result, anyhow};
use dusk_protocol::client::{Event, Request, Response, Status};
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

/// Where the events of open threads go: one queue for each attached client.
pub type Subscribers = Arc<Mutex<Vec<(ClientId, mpsc::UnboundedSender<Event>)>>>;

pub enum Message {
    /// A client joined; `source` is the name its threads carry.
    Attach {
        client: ClientId,
        source: String,
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
}

pub struct Server {
    runtime: Arc<Runtime>,
    /// The name of each attached client, the source of the threads it creates.
    sources: HashMap<ClientId, String>,
    /// The client whose request is being answered.
    caller: ClientId,
    subscribers: Subscribers,
    status: Arc<watch::Sender<Status>>,
    threads: HashMap<String, OpenThread>,
    terminals: Terminals,
}

impl Server {
    pub fn new(runtime: Runtime, subscribers: Subscribers) -> (Self, watch::Receiver<Status>) {
        let status = read_status(&runtime, None);
        let (sender, receiver) = watch::channel(status);
        let server = Self {
            runtime: Arc::new(runtime),
            sources: HashMap::new(),
            caller: 0,
            subscribers,
            status: Arc::new(sender),
            threads: HashMap::new(),
            terminals: Terminals::new(crate::terminal::login_shell()),
        };
        (server, receiver)
    }

    /// The name of the client being answered.
    pub(super) fn source(&self) -> &str {
        self.sources
            .get(&self.caller)
            .map_or("client", String::as_str)
    }

    /// Answers calls until the client asks to shut down or goes away, then closes every
    /// thread and stops the runtime.
    pub async fn run(mut self, mut calls: mpsc::UnboundedReceiver<Message>) {
        let mut clock = tokio::time::interval(STATUS_PERIOD);
        clock.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        // The first tick is at once, so runs missed while the app was closed start now.
        let mut timetable = tokio::time::interval(SCHEDULE_PERIOD);
        timetable.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let last = loop {
            tokio::select! {
                call = calls.recv() => match call {
                    None => break None,
                    Some(Message::Attach { client, source }) => {
                        self.sources.insert(client, source);
                    }
                    Some(Message::Detach(client)) => self.detach(client).await,
                    Some(Message::Call(Call { request: Request::Shutdown, reply, .. })) => {
                        break Some(reply);
                    }
                    Some(Message::Call(Call { request: Request::InstallUpdate { tag }, reply, .. })) => {
                        self.install_update(tag, reply);
                    }
                    Some(Message::Call(Call { request: Request::Simulator(request), reply, .. })) => {
                        simulator::answer(request, reply);
                    }
                    Some(Message::Call(Call { request: Request::Store(request), reply, .. })) => {
                        self.store(request, reply);
                    }
                    Some(Message::Call(Call { client, request, reply })) => {
                        self.caller = client;
                        let response = match self.handle(request).await {
                            Ok(response) => response,
                            Err(error) => Response::Error(format!("{error:#}")),
                        };
                        // The client reads the status once it has the answer.
                        self.refresh_status();
                        let _ = reply.send(response);
                    }
                },
                _ = clock.tick() => {
                    self.refresh_status();
                    self.close_orphans().await;
                }
                _ = timetable.tick() => self.run_due_schedules().await,
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
            Request::Plugin(request) => self.plugin(request),
            Request::InstallUpdate { .. }
            | Request::Simulator(_)
            | Request::Store(_)
            | Request::Shutdown => Err(anyhow!("this request is answered by the loop")),
        }
    }
}
