//! The bridge from the window to the core. The window's executor awaits the calls;
//! the core itself runs on the tokio runtime it was started on.

use std::{future::Future, sync::Arc};

use anyhow::Result;
use dusk_core::client::Client;
use dusk_protocol::client::{Event, Status};
use gpui_kit::*;
use tokio::sync::{mpsc, watch};

/// Takes the event and status streams out, so the calls can be shared.
pub fn split(
    mut client: Client,
) -> (
    Arc<Client>,
    mpsc::UnboundedReceiver<Event>,
    watch::Receiver<Status>,
) {
    let (_, empty) = mpsc::unbounded_channel();
    let events = std::mem::replace(&mut client.events, empty);
    let status = client.status.clone();
    (Arc::new(client), events, status)
}

#[derive(Clone)]
pub struct Core {
    client: Arc<Client>,
    runtime: Arc<tokio::runtime::Runtime>,
}

impl Core {
    pub fn new(client: Arc<Client>, runtime: Arc<tokio::runtime::Runtime>) -> Self {
        Self { client, runtime }
    }

    /// Runs `call` against the core, then `done` on the view with its result.
    pub fn call<V, T, F, Fut>(
        &self,
        cx: &mut Context<V>,
        call: F,
        done: impl FnOnce(&mut V, Result<T>, &mut Context<V>) + 'static,
    ) where
        V: 'static,
        T: Send + 'static,
        F: FnOnce(Arc<Client>) -> Fut + Send + 'static,
        Fut: Future<Output = Result<T>> + Send + 'static,
    {
        // Calls run on the core's runtime: some of them spawn tasks of their own.
        let task = self.runtime.spawn(call(Arc::clone(&self.client)));
        cx.spawn(async move |view, cx| {
            let result = task
                .await
                .unwrap_or_else(|error| Err(anyhow::anyhow!("the call failed: {error}")));
            let _ = view.update(cx, |view, cx| done(view, result, cx));
        })
        .detach();
    }
}
