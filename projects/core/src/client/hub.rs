//! The hub: the running core and the clients attached to it.

use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

use anyhow::{Result, anyhow, ensure};
use dusk_protocol::client::{Busy, Hello, PROTOCOL_VERSION, Status};
use parking_lot::Mutex;
use tokio::sync::{mpsc, oneshot, watch};

use super::{
    Client, Scope,
    connection::Link,
    serve::{Message, Server, Subscriber, Subscribers, blank_status, stopping},
};
use crate::runtime::Runtime;

/// The running core, which any number of clients attach to. A client sees the events of
/// the open threads its scope covers; the core stops when the last client and the hub are
/// gone, or when one of them asks it to shut down.
pub struct Hub {
    pub(super) calls: mpsc::UnboundedSender<Message>,
    subscribers: Subscribers,
    status: watch::Receiver<Status>,
    /// What a client gets whose scope does not reach the status; kept so it stays open.
    blank: watch::Sender<Status>,
    next_client: AtomicU64,
    busy: watch::Receiver<Busy>,
    /// The version of a newer app that asked the owner to hand the core over.
    handover: watch::Sender<Option<String>>,
    answer: Mutex<Option<oneshot::Sender<bool>>>,
}

impl Hub {
    pub fn start(runtime: Runtime) -> Arc<Self> {
        let (calls, inbox) = mpsc::unbounded_channel();
        let subscribers = Subscribers::default();
        let (server, status) = Server::new(runtime, Arc::clone(&subscribers));
        let busy = server.busy();
        tokio::spawn(server.run(inbox));
        Arc::new(Self {
            calls,
            subscribers,
            status,
            blank: watch::channel(blank_status()).0,
            next_client: AtomicU64::new(1),
            busy,
            handover: watch::channel(None).0,
            answer: Mutex::new(None),
        })
    }

    /// The status as it is now.
    pub fn status(&self) -> Status {
        self.status.borrow().clone()
    }

    /// How much the core is doing: the clients but the owner, and the open threads with a
    /// turn running.
    pub fn busy(&self) -> watch::Receiver<Busy> {
        self.busy.clone()
    }

    /// Tells every client the core stops for `reason`, then stops it as `Request::Shutdown`
    /// does: every thread is closed and the runtime shut down.
    pub fn stop(&self, reason: &str) {
        stopping(&self.subscribers, reason);
        let _ = self.calls.send(Message::Stop);
    }

    /// A newer app of `version` asks the owner to hand the core over; true once the owner
    /// agrees. A later ask replaces this one, which then reads as refused.
    pub fn ask_handover(&self, version: String) -> oneshot::Receiver<bool> {
        let (reply, receiver) = oneshot::channel();
        *self.answer.lock() = Some(reply);
        self.handover.send_replace(Some(version));
        receiver
    }

    /// The handover asked of the owner, by version, until it is answered.
    pub fn handover_requests(&self) -> watch::Receiver<Option<String>> {
        self.handover.subscribe()
    }

    /// The owner's answer to the handover asked.
    pub fn answer_handover(&self, accept: bool) {
        if let Some(reply) = self.answer.lock().take() {
            let _ = reply.send(accept);
        }
        self.handover.send_replace(None);
    }

    /// Attaches the client that said `hello`, to see and ask what `scope` allows. The
    /// scope is the caller's decision, never the client's, and holds until the client is
    /// dropped: dropping it is how access is taken back.
    pub fn attach(self: &Arc<Self>, hello: Hello, scope: Scope) -> Result<Client> {
        self.attach_as(hello, scope, "in-process", false)
    }

    /// Attaches the app that runs this core: the one `busy` does not count. Only the
    /// first owner attached holds it, until it goes.
    pub fn attach_owner(self: &Arc<Self>, hello: Hello, scope: Scope) -> Result<Client> {
        self.attach_as(hello, scope, "in-process", true)
    }

    /// As `attach`, for a client that arrived over `transport`, which the clients list names.
    pub fn attach_labelled(
        self: &Arc<Self>,
        hello: Hello,
        scope: Scope,
        transport: &'static str,
    ) -> Result<Client> {
        self.attach_as(hello, scope, transport, false)
    }

    fn attach_as(
        self: &Arc<Self>,
        hello: Hello,
        scope: Scope,
        transport: &'static str,
        owner: bool,
    ) -> Result<Client> {
        // In-process clients are built with this core, so they speak its version exactly.
        ensure!(
            hello.protocol == PROTOCOL_VERSION,
            "the client speaks protocol {}, the core {PROTOCOL_VERSION}",
            hello.protocol
        );
        hello.accepted().map_err(|refusal| anyhow!("{refusal}"))?;
        let id = self.next_client.fetch_add(1, Ordering::Relaxed);
        let (queue, events) = mpsc::unbounded_channel();
        // Subscribed before the core hears of the client, so no event is missed.
        self.subscribers.lock().push(Subscriber {
            client: id,
            scope: scope.clone(),
            queue,
        });
        let reads_status = scope.reads_status();
        let status = match reads_status {
            true => self.status.clone(),
            false => self.blank.subscribe(),
        };
        let attach = Message::Attach {
            client: id,
            source: hello.client,
            scope,
            transport,
            owner,
        };
        ensure!(self.calls.send(attach).is_ok(), "the core has stopped");
        Ok(Client {
            events,
            status,
            reads_status,
            link: Link::Local {
                id,
                hub: Arc::clone(self),
            },
        })
    }
}
