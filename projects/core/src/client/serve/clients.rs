//! Who is attached to the core and how busy it is: the clients list, disconnecting one
//! and the count the owner reads before it lets a core go.

use super::{ClientId, Server};
use anyhow::{Result, anyhow};
use dusk_protocol::client::{AttachedClient, Busy, Response};
use tokio::sync::watch;

impl Server {
    /// The clients attached now, oldest first.
    pub(super) fn attached(&self) -> Vec<AttachedClient> {
        let mut listed: Vec<AttachedClient> = self
            .clients
            .iter()
            .map(|(id, attached)| AttachedClient {
                id: *id,
                label: attached.source.clone(),
                transport: attached.transport.to_owned(),
                scope: attached.scope.name().to_owned(),
                since: attached.since,
            })
            .collect();
        listed.sort_by_key(|client| client.id);
        listed
    }

    /// What `client` called itself, for the reason a stop carries.
    pub(super) fn label(&self, client: ClientId) -> &str {
        self.clients
            .get(&client)
            .map_or("a client", |attached| attached.source.as_str())
    }

    /// Detaches `id`; its event queue closes.
    pub(super) async fn disconnect(&mut self, id: ClientId) -> Result<Response> {
        if !self.clients.contains_key(&id) {
            return Err(anyhow!("no such client"));
        }
        self.detach(id).await;
        self.refresh_busy();
        Ok(Response::Done)
    }

    /// The clients but the owner, and the open threads with a turn running, unattended
    /// ones too.
    pub(super) fn refresh_busy(&self) {
        let others = self
            .clients
            .keys()
            .filter(|id| Some(**id) != self.owner)
            .count();
        let busy = Busy {
            clients: others as u32,
            running: self
                .threads
                .values()
                .filter(|open| open.running.lock().is_some())
                .count() as u32,
        };
        self.busy.send_if_modified(|old| {
            let changed = *old != busy;
            *old = busy;
            changed
        });
    }

    pub fn busy(&self) -> watch::Receiver<Busy> {
        self.busy.subscribe()
    }
}
