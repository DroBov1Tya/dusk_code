//! `Client`: one client's connection to the core, and the calls that belong to no area.

use std::{future::Future, path::PathBuf, sync::Arc};

use anyhow::{Result, anyhow, bail};
use dusk_protocol::{
    catalog::NodeText,
    client::{Answer, Command, Event, Request, Response, Status},
    system::SystemReport,
    thread::ContextReport,
};
use tokio::{
    io::{AsyncBufRead, AsyncWrite},
    sync::{mpsc, oneshot, watch},
};

use super::{
    Catalog, ClientId, Hub, Plugins, Projects, Settings, Terminal, Threads, Work, remote,
    serve::{Call, Message, blank_status},
};

pub struct Client {
    /// The events of the open threads, in the order they happened.
    pub events: mpsc::UnboundedReceiver<Event>,
    /// The latest status; `changed` wakes when the core sends a new one.
    pub status: watch::Receiver<Status>,
    /// Whether `status` is the core's: false when the scope may not read it, and the
    /// status is then a blank that never changes.
    pub(super) reads_status: bool,
    pub(super) link: Link,
}

/// Where the calls of a client go: into the hub of this process, or over a socket.
pub(super) enum Link {
    Local { id: ClientId, hub: Arc<Hub> },
    Remote(remote::Link),
}

impl Drop for Client {
    fn drop(&mut self) {
        // A remote client leaves by closing the socket, which its link does on drop.
        if let Link::Local { id, hub } = &self.link {
            let _ = hub.calls.send(Message::Detach(*id));
        }
    }
}

impl Client {
    /// A client of a core on the other end of a socket, after the handshake succeeded.
    /// It lives on its own tasks; they end with the socket or when the client drops.
    pub async fn over(
        read: impl AsyncBufRead + Unpin + Send + 'static,
        write: impl AsyncWrite + Unpin + Send + 'static,
    ) -> Client {
        let (events_in, events) = mpsc::unbounded_channel();
        let (status_in, status) = watch::channel(blank_status());
        let link = remote::Link::start(read, write, events_in, status_in);
        Client {
            events,
            status,
            // An app attaches with the full scope, and the core sends it every status.
            reads_status: true,
            link: Link::Remote(link),
        }
    }

    /// Whether `status` is the core's; false when it is a blank that never changes.
    pub fn reads_status(&self) -> bool {
        self.reads_status
    }

    /// The id the core knows this client by; none for a client over a socket, which
    /// learned its id in the handshake.
    pub fn id(&self) -> Option<ClientId> {
        match &self.link {
            Link::Local { id, .. } => Some(*id),
            Link::Remote(_) => None,
        }
    }

    /// The hub of this process; none for a client over a socket.
    pub fn hub(&self) -> Option<Arc<Hub>> {
        match &self.link {
            Link::Local { hub, .. } => Some(Arc::clone(hub)),
            Link::Remote(_) => None,
        }
    }

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

    pub fn terminal(&self) -> Terminal<'_> {
        Terminal(self)
    }

    pub fn plugins(&self) -> Plugins<'_> {
        Plugins(self)
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

    pub(super) async fn call(&self, request: Request) -> Result<Response> {
        answer(self.send(request).await)
    }

    pub(super) async fn done(&self, request: Request) -> Result<()> {
        match self.call(request).await? {
            Response::Done => Ok(()),
            other => Err(unexpected(other)),
        }
    }

    pub(super) async fn text(&self, request: Request) -> Result<String> {
        match self.call(request).await? {
            Response::Text(text) => Ok(text),
            other => Err(unexpected(other)),
        }
    }

    pub(super) async fn node(&self, request: Request) -> Result<NodeText> {
        match self.call(request).await? {
            Response::Node(node) => Ok(node),
            other => Err(unexpected(other)),
        }
    }

    fn send(&self, request: Request) -> oneshot::Receiver<Response> {
        match &self.link {
            Link::Local { id, hub } => {
                let (reply, answer) = oneshot::channel();
                // A stopped core drops the call, and the receiver says so.
                let call = Call {
                    client: *id,
                    request,
                    reply,
                };
                let _ = hub.calls.send(Message::Call(call));
                answer
            }
            Link::Remote(link) => link.send(request),
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

pub(super) fn unexpected(response: Response) -> anyhow::Error {
    anyhow!("the core gave an unexpected answer: {response:?}")
}
