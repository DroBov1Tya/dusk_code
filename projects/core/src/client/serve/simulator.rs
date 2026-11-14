//! The iOS Simulator, answered beside the loop rather than in it: simctl takes from a
//! fraction of a second for a screen to minutes for a first boot.

use anyhow::Result;
use dusk_protocol::client::{Response, SimulatorRequest};
use tokio::sync::oneshot;

pub(super) fn answer(request: SimulatorRequest, reply: oneshot::Sender<Response>) {
    tokio::spawn(async move {
        let response = match on_this_system(request).await {
            Ok(response) => response,
            Err(error) => Response::Error(format!("{error:#}")),
        };
        let _ = reply.send(response);
    });
}

#[cfg(target_os = "macos")]
async fn on_this_system(request: SimulatorRequest) -> Result<Response> {
    crate::platform::simulator::answer(request).await
}

#[cfg(not(target_os = "macos"))]
async fn on_this_system(_: SimulatorRequest) -> Result<Response> {
    anyhow::bail!("the iOS Simulator runs on macOS only")
}
