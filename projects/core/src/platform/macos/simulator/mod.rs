//! Xcode's iOS Simulator, for the desktop app's panel and the agent's tool. simctl
//! lists, boots and pictures the devices; AXe, when installed, touches and types on
//! them. A device is named only by a UDID that parses as one.
//!
//! - `run`: the programs, without a shell;
//! - `listing`: Xcode, the runtimes and the devices;
//! - `download`: the iOS runtime;
//! - `screen`: a device's picture and its size in points;
//! - `stream`: its screen as it changes, for the panel;
//! - `input`: touches, typing, buttons and the accessibility tree;
//! - `tree`: that tree made short for the model;
//! - `apps`: links, installed apps and Simulator.app.

mod apps;
mod download;
mod input;
mod listing;
mod run;
mod screen;
mod stream;
mod tree;

use anyhow::Result;
use dusk_protocol::client::{Response, SimulatorRequest};

pub use apps::{launch, open_url};
pub use input::{button, swipe, tap, tree, type_text};
pub use listing::booted;
pub use screen::take as screen;

/// A first boot builds the device's data and can take this long.
const BOOT_LIMIT: std::time::Duration = std::time::Duration::from_secs(180);

pub async fn answer(request: SimulatorRequest) -> Result<Response> {
    use SimulatorRequest as Ask;
    Ok(match request {
        Ask::Look => Response::Simulator(listing::look().await?),
        Ask::InstallRuntime => {
            download::start();
            Response::Done
        }
        Ask::OpenApp => {
            apps::open_simulator_app().await?;
            Response::Done
        }
        Ask::Boot { udid } => {
            let udid = run::device(&udid)?;
            settle(run::simctl(&["boot", &udid], BOOT_LIMIT).await, "Booted")?;
            // `boot` returns before the device can draw; a picture taken then fails.
            run::simctl(&["bootstatus", &udid, "-b"], BOOT_LIMIT).await?;
            Response::Done
        }
        Ask::Shutdown { udid } => {
            let udid = run::device(&udid)?;
            settle(
                run::simctl(&["shutdown", &udid], run::LIMIT).await,
                "Shutdown",
            )?;
            Response::Done
        }
        Ask::Screen { udid, after } => {
            let udid = run::device(&udid)?;
            Response::SimScreen(stream::next(&udid, after, listing::axe_found()).await?)
        }
        Ask::Tap { udid, x, y } => {
            input::tap(&run::device(&udid)?, x, y).await?;
            Response::Done
        }
        Ask::Swipe { udid, from, to } => {
            input::swipe(&run::device(&udid)?, from, to).await?;
            Response::Done
        }
        Ask::Type { udid, text } => {
            input::type_text(&run::device(&udid)?, &text).await?;
            Response::Done
        }
        Ask::Button { udid, button } => {
            input::button(&run::device(&udid)?, button).await?;
            Response::Done
        }
    })
}

/// A device already in the state asked for is not a failure.
fn settle(result: Result<Vec<u8>>, state: &str) -> Result<()> {
    match result {
        Err(error) if format!("{error:#}").contains(&format!("current state: {state}")) => Ok(()),
        other => other.map(drop),
    }
}
