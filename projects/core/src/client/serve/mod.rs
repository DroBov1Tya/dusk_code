//! The core's side of the protocol: one task that owns the runtime, answers requests in
//! the order they come, forwards the events of open threads and keeps the status fresh.

mod catalog;
mod projects;
mod settings;
mod status;
mod thread;
mod voice;
mod work;

use status::read_status;

use crate::{
    message::commands,
    runtime::{Runtime, ThreadInput},
};
use anyhow::{Result, anyhow};
use dusk_protocol::client::{Event, Request, Response, Status};
use std::{collections::HashMap, sync::Arc, time::Duration};
use tokio::{
    sync::{mpsc, oneshot, watch},
    task::JoinHandle,
};
use tracing::warn;

/// How long a closing thread may take to stop its turn and finish.
const CLOSE_TIMEOUT: Duration = Duration::from_secs(10);

/// How often the status is read again for what changes in the background, such as a
/// model found over the network or an MCP server that settled.
const STATUS_PERIOD: Duration = Duration::from_millis(100);

pub struct Call {
    pub request: Request,
    pub reply: oneshot::Sender<Response>,
}

/// An open thread: where its input goes and the task that forwards its events.
struct OpenThread {
    input: ThreadInput,
    forward: JoinHandle<()>,
}

pub struct Server {
    runtime: Arc<Runtime>,
    /// The client's name, the source of the threads it creates.
    source: String,
    events: mpsc::UnboundedSender<Event>,
    status: Arc<watch::Sender<Status>>,
    threads: HashMap<String, OpenThread>,
}

impl Server {
    pub fn new(
        runtime: Runtime,
        source: String,
        events: mpsc::UnboundedSender<Event>,
    ) -> (Self, watch::Receiver<Status>) {
        let status = read_status(&runtime, None);
        let (sender, receiver) = watch::channel(status);
        let server = Self {
            runtime: Arc::new(runtime),
            source,
            events,
            status: Arc::new(sender),
            threads: HashMap::new(),
        };
        (server, receiver)
    }

    /// Answers calls until the client asks to shut down or goes away, then closes every
    /// thread and stops the runtime.
    pub async fn run(mut self, mut calls: mpsc::UnboundedReceiver<Call>) {
        let mut clock = tokio::time::interval(STATUS_PERIOD);
        clock.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let last = loop {
            tokio::select! {
                call = calls.recv() => match call {
                    None => break None,
                    Some(Call { request: Request::Shutdown, reply }) => break Some(reply),
                    Some(Call { request: Request::InstallUpdate { tag }, reply }) => {
                        self.install_update(tag, reply);
                    }
                    Some(Call { request, reply }) => {
                        let response = match self.handle(request).await {
                            Ok(response) => response,
                            Err(error) => Response::Error(format!("{error:#}")),
                        };
                        // The client reads the status once it has the answer.
                        self.refresh_status();
                        let _ = reply.send(response);
                    }
                },
                _ = clock.tick() => self.refresh_status(),
            }
        };
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
            Request::Project(request) => self.project(request).await,
            Request::Command(command) => Ok(Response::Answer(Box::new(
                commands::answer(command, &self.runtime).await?,
            ))),
            Request::Voice(request) => self.voice(request).await,
            Request::SystemReport => Ok(Response::System(self.runtime.system_report().await?)),
            Request::InstallUpdate { .. } | Request::Shutdown => {
                Err(anyhow!("this request is answered by the loop"))
            }
        }
    }
}
