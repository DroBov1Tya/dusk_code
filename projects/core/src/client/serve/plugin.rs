//! Requests about the parts plugins add to the window. A part belongs to the clients
//! that opened it: its views and frames go to them alone, as a terminal's bytes do.

use super::Server;
use anyhow::{Context, Result, ensure};
use dusk_protocol::{
    client::{Event, PluginRequest, Response},
    plugin::StoreRequest,
};
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot};

impl Server {
    /// Answers at once: starting an interface spawns its process and returns, and its
    /// view comes later as an event.
    pub(super) async fn plugin(&self, request: PluginRequest) -> Result<Response> {
        let host = self.runtime.plugin_ui();
        match request {
            PluginRequest::Redetect { plugin, part } => {
                ensure!(
                    crate::plugins::ui::PhoneDependency::for_panel(&plugin, &part).is_some(),
                    "this is not a phone dependency panel"
                );
                let previous = host.generation(&plugin);
                self.runtime.scan_plugins().await?;
                host.redetect(self.caller, self.plugin_queue()?, &plugin, &part, previous)?;
            }
            PluginRequest::Open { plugin, part } => {
                host.open(self.caller, self.plugin_queue()?, &plugin, &part)?;
            }
            PluginRequest::Close { plugin, part } => host.close(self.caller, &plugin, &part),
            PluginRequest::Event {
                plugin,
                part,
                widget,
                event,
            } => host.event(self.caller, &plugin, &part, &widget, event)?,
        }
        Ok(Response::Done)
    }

    fn plugin_queue(&self) -> Result<mpsc::UnboundedSender<Event>> {
        self.subscribers
            .lock()
            .iter()
            .find(|(client, _)| *client == self.caller)
            .map(|(_, queue)| queue.clone())
            .context("the client is gone")
    }
}

impl Server {
    /// Answers beside the loop: the store is on the network, and an install may take a
    /// while; the plugins list in the status shows the result.
    pub(super) fn store(&self, request: StoreRequest, reply: oneshot::Sender<Response>) {
        let runtime = Arc::clone(&self.runtime);
        tokio::spawn(async move {
            let response = match runtime.store(request).await {
                Ok(response) => response,
                Err(error) => Response::Error(format!("{error:#}")),
            };
            let _ = reply.send(response);
        });
    }
}
