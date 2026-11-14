//! The hub: the running core and the clients attached to it.

use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

use anyhow::{Result, ensure};
use dusk_protocol::client::{Hello, PROTOCOL_VERSION, Status};
use tokio::sync::{mpsc, watch};

use super::{
    Client,
    serve::{Message, Server, Subscribers},
};
use crate::runtime::Runtime;

/// The running core, which any number of clients attach to. Every client sees the events
/// of every open thread; the core stops when the last client and the hub are gone, or
/// when one of them asks it to shut down.
pub struct Hub {
    pub(super) calls: mpsc::UnboundedSender<Message>,
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
