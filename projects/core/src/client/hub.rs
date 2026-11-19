//! The hub: the running core and the clients attached to it.

use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

use anyhow::{Result, anyhow, ensure};
use dusk_protocol::client::{Hello, PROTOCOL_VERSION, Status};
use tokio::sync::{mpsc, watch};

use super::{
    Client, Scope,
    connection::Link,
    serve::{Message, Server, Subscriber, Subscribers, blank_status},
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
            blank: watch::channel(blank_status()).0,
            next_client: AtomicU64::new(1),
        })
    }

    /// The status as it is now.
    pub fn status(&self) -> Status {
        self.status.borrow().clone()
    }

    /// Attaches the client that said `hello`, to see and ask what `scope` allows. The
    /// scope is the caller's decision, never the client's, and holds until the client is
    /// dropped: dropping it is how access is taken back.
    pub fn attach(self: &Arc<Self>, hello: Hello, scope: Scope) -> Result<Client> {
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
