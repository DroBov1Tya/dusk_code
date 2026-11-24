//! The bridge from the window to the core. The window's executor awaits the calls;
//! the core itself runs on the tokio runtime it was started on.

use std::{
    future::Future,
    sync::{Arc, Mutex},
};

use anyhow::Result;
use dusk_core::client::{Client, Hub};
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

/// This process's hold on the data under HOME, which a removal must take over.
pub type Hold = Arc<Mutex<Option<dusk_core::app::RestoreGuard>>>;

#[derive(Clone)]
pub struct Core {
    client: Arc<Client>,
    runtime: Arc<tokio::runtime::Runtime>,
    /// The running core, which other clients such as the web server attach to.
    hub: Arc<Hub>,
    hold: Hold,
}

impl Core {
    pub fn new(
        client: Arc<Client>,
        runtime: Arc<tokio::runtime::Runtime>,
        hub: Arc<Hub>,
        hold: Hold,
    ) -> Self {
        Self {
            client,
            runtime,
            hub,
            hold,
        }
    }

    /// Takes duskcode off this computer from inside the app: the core stops, this
    /// process lets go of the data, and the core's removal (the one `duskcode remove`
    /// runs) deletes it. `done` hears the keys the keychain would not give up, if any.
    /// The core is stopped either way, so the window can only close after it.
    pub fn uninstall<V: 'static>(
        &self,
        cx: &mut Context<V>,
        files: bool,
        done: impl FnOnce(&mut V, Result<Option<String>>, &mut Context<V>) + 'static,
    ) {
        let (client, hold) = (Arc::clone(&self.client), Arc::clone(&self.hold));
        let task = self.runtime.spawn(async move {
            let stop = client.shutdown();
            let _ = tokio::time::timeout(std::time::Duration::from_secs(10), stop).await;
            hold.lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .take();
            tokio::task::spawn_blocking(move || {
                let paths = dusk_core::app::Paths::from_home()?;
                let mut kept = None;
                dusk_core::app::Uninstall::plan(&paths, files, true)?.run(|error| {
                    tracing::warn!("keys left in the keychain: {error:#}");
                    kept = Some(format!("{error:#}"));
                })?;
                Ok(kept)
            })
            .await
            .unwrap_or_else(|error| Err(anyhow::anyhow!("the removal stopped: {error}")))
        });
        cx.spawn(async move |view, cx| {
            let result = task
                .await
                .unwrap_or_else(|error| Err(anyhow::anyhow!("the removal stopped: {error}")));
            let _ = view.update(cx, |view, cx| done(view, result, cx));
        })
        .detach();
    }

    /// Like `call`, for work on the hub itself, such as starting the web server.
    pub fn call_hub<V, T, F, Fut>(
        &self,
        cx: &mut Context<V>,
        call: F,
        done: impl FnOnce(&mut V, Result<T>, &mut Context<V>) + 'static,
    ) where
        V: 'static,
        T: Send + 'static,
        F: FnOnce(Arc<Hub>) -> Fut + Send + 'static,
        Fut: Future<Output = Result<T>> + Send + 'static,
    {
        let task = self.runtime.spawn(call(Arc::clone(&self.hub)));
        cx.spawn(async move |view, cx| {
            let result = task
                .await
                .unwrap_or_else(|error| Err(anyhow::anyhow!("the call failed: {error}")));
            let _ = view.update(cx, |view, cx| done(view, result, cx));
        })
        .detach();
    }

    /// Starts work against the core that lives as long as it has something to do, such
    /// as a queue of calls that must be made in order.
    pub fn run<F, Fut>(&self, work: F)
    where
        F: FnOnce(Arc<Client>) -> Fut + Send + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        self.runtime.spawn(work(Arc::clone(&self.client)));
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
