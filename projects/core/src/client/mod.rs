use crate::runtime::Runtime;
use anyhow::{Result, anyhow, bail, ensure};
use dusk_protocol::{
    catalog::NodeText,
    client::{Answer, Command, Event, Hello, PROTOCOL_VERSION, Request, Response, Status},
    system::SystemReport,
    thread::ContextReport,
};
use std::{future::Future, path::PathBuf};
use tokio::sync::{mpsc, oneshot, watch};

mod catalog;
mod projects;
mod serve;
mod settings;
mod threads;
mod work;

use serve::{Call, Server};

pub use catalog::Catalog;
pub use projects::Projects;
pub use settings::Settings;
pub use threads::Threads;
pub use work::Work;

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

    /// What the next request of `thread_id` carries, with the per-item breakdown.
    pub async fn context_report(&self, thread_id: Option<String>) -> Result<ContextReport> {
        match self.command(Command::Context { thread_id }).await? {
            Answer::Context(report) => Ok(report),
            other => Err(anyhow!(
                "the command answered {other:?}, not a context report"
            )),
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

    /// The text of a WAV recording, by the speech recognition server.
    pub async fn transcribe(&self, audio: Vec<u8>) -> Result<String> {
        let request = Request::Voice(dusk_protocol::client::VoiceRequest::Transcribe { audio });
        match self.call(request).await? {
            Response::Text(text) => Ok(text),
            other => Err(unexpected(other)),
        }
    }

    /// Reads a text aloud; the sound is a card of the artifact store.
    pub async fn speak(&self, text: &str) -> Result<dusk_protocol::tools::Card> {
        let request = Request::Voice(dusk_protocol::client::VoiceRequest::Speak {
            text: text.to_owned(),
        });
        match self.call(request).await? {
            Response::Card(card) => Ok(card),
            other => Err(unexpected(other)),
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
