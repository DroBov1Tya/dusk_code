use crate::runtime::Runtime;
use anyhow::{Result, anyhow, bail, ensure};
use dusk_protocol::{
    catalog::NodeText,
    client::{Answer, Command, Event, Hello, PROTOCOL_VERSION, Request, Response, Status},
    system::SystemReport,
    thread::ContextReport,
};
use std::{
    future::Future,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};
use tokio::sync::{mpsc, oneshot, watch};

mod catalog;
mod projects;
mod serve;
mod settings;
mod threads;
mod work;

use serve::{Call, ClientId, Message, Server, Subscribers};

pub use catalog::Catalog;
pub use projects::Projects;
pub use settings::Settings;
pub use threads::Threads;
pub use work::Work;

/// Starts the core's side of the protocol over `runtime` for the one client that said
/// `hello`. The core stops when that client goes away.
pub fn serve(runtime: Runtime, hello: Hello) -> Result<Client> {
    Hub::start(runtime).attach(hello)
}

/// The running core, which any number of clients attach to. Every client sees the events
/// of every open thread; the core stops when the last client and the hub are gone, or
/// when one of them asks it to shut down.
pub struct Hub {
    calls: mpsc::UnboundedSender<Message>,
    subscribers: Subscribers,
    status: watch::Receiver<Status>,
    next_client: AtomicU64,
}

impl Hub {
    pub fn start(runtime: Runtime) -> Arc<Self> {
        let (calls, inbox) = mpsc::unbounded_channel();
        let subscribers = Subscribers::default();
        let (server, status) = Server::new(runtime, Arc::clone(&subscribers));
        tokio::spawn(server.run(inbox));
        Arc::new(Self {
            calls,
            subscribers,
            status,
            next_client: AtomicU64::new(1),
        })
    }

    /// The status as it is now.
    pub fn status(&self) -> Status {
        self.status.borrow().clone()
    }

    /// Attaches the client that said `hello`.
    pub fn attach(self: &Arc<Self>, hello: Hello) -> Result<Client> {
        ensure!(
            hello.protocol == PROTOCOL_VERSION,
            "the client speaks protocol {}, the core {PROTOCOL_VERSION}",
            hello.protocol
        );
        let id = self.next_client.fetch_add(1, Ordering::Relaxed);
        let (queue, events) = mpsc::unbounded_channel();
        // Subscribed before the core hears of the client, so no event is missed.
        self.subscribers.lock().push((id, queue));
        let attach = Message::Attach {
            client: id,
            source: hello.client,
        };
        ensure!(self.calls.send(attach).is_ok(), "the core has stopped");
        Ok(Client {
            events,
            status: self.status.clone(),
            id,
            hub: Arc::clone(self),
        })
    }
}

pub struct Client {
    /// The events of the open threads, in the order they happened.
    pub events: mpsc::UnboundedReceiver<Event>,
    /// The latest status; `changed` wakes when the core sends a new one.
    pub status: watch::Receiver<Status>,
    id: ClientId,
    hub: Arc<Hub>,
}

impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.hub.calls.send(Message::Detach(self.id));
    }
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

    /// The speech models of this machine and where each is.
    pub async fn voice_models(&self) -> Result<dusk_protocol::config::VoiceModels> {
        let request = Request::Voice(dusk_protocol::client::VoiceRequest::Models);
        match self.call(request).await? {
            Response::VoiceModels(models) => Ok(models),
            other => Err(unexpected(other)),
        }
    }

    /// Installs a model, starts it and sets the speech to it. Returns when it is begun.
    pub async fn install_voice_model(&self, id: &str) -> Result<()> {
        let request =
            Request::Voice(dusk_protocol::client::VoiceRequest::Install { id: id.to_owned() });
        match self.call(request).await? {
            Response::Done => Ok(()),
            other => Err(unexpected(other)),
        }
    }

    pub async fn cancel_voice_model(&self, id: &str) -> Result<()> {
        let request =
            Request::Voice(dusk_protocol::client::VoiceRequest::Cancel { id: id.to_owned() });
        match self.call(request).await? {
            Response::Done => Ok(()),
            other => Err(unexpected(other)),
        }
    }

    pub async fn remove_voice_model(&self, id: &str) -> Result<()> {
        let request =
            Request::Voice(dusk_protocol::client::VoiceRequest::Remove { id: id.to_owned() });
        match self.call(request).await? {
            Response::Done => Ok(()),
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

    /// Any request as it comes, for a bridge to a remote client; a failure is the
    /// `Response::Error` the core would give.
    pub async fn request(&self, request: Request) -> Response {
        match self.send(request).await {
            Ok(response) => response,
            Err(_) => Response::Error("the core has stopped".to_owned()),
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
        let call = Call {
            client: self.id,
            request,
            reply,
        };
        let _ = self.hub.calls.send(Message::Call(call));
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
