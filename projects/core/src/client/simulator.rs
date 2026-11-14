//! Xcode's iOS Simulator, for the desktop app on macOS.

use super::{Client, unexpected};
use anyhow::Result;
use dusk_protocol::{
    client::{Request, Response, SimulatorRequest},
    system::{SimButton, SimScreen, Simulator as Found},
};

pub struct Simulator<'a>(pub(super) &'a Client);

impl Simulator<'_> {
    /// What is installed and which devices there are.
    pub async fn look(&self) -> Result<Found> {
        match self.ask(SimulatorRequest::Look).await? {
            Response::Simulator(found) => Ok(found),
            other => Err(unexpected(other)),
        }
    }

    /// The device's screen and its size in points.
    /// The device's screen: the first frame after `after`, or the last one when it
    /// holds still.
    pub async fn screen(&self, udid: String, after: u64) -> Result<SimScreen> {
        match self.ask(SimulatorRequest::Screen { udid, after }).await? {
            Response::SimScreen(screen) => Ok(screen),
            other => Err(unexpected(other)),
        }
    }

    /// Starts downloading the iOS runtime; `look` tells when it is done.
    pub async fn install_runtime(&self) -> Result<()> {
        self.done(SimulatorRequest::InstallRuntime).await
    }

    pub async fn boot(&self, udid: String) -> Result<()> {
        self.done(SimulatorRequest::Boot { udid }).await
    }

    pub async fn shutdown(&self, udid: String) -> Result<()> {
        self.done(SimulatorRequest::Shutdown { udid }).await
    }

    pub async fn tap(&self, udid: String, x: u32, y: u32) -> Result<()> {
        self.done(SimulatorRequest::Tap { udid, x, y }).await
    }

    pub async fn swipe(&self, udid: String, from: (u32, u32), to: (u32, u32)) -> Result<()> {
        self.done(SimulatorRequest::Swipe { udid, from, to }).await
    }

    pub async fn type_text(&self, udid: String, text: String) -> Result<()> {
        self.done(SimulatorRequest::Type { udid, text }).await
    }

    pub async fn button(&self, udid: String, button: SimButton) -> Result<()> {
        self.done(SimulatorRequest::Button { udid, button }).await
    }

    pub async fn open_app(&self) -> Result<()> {
        self.done(SimulatorRequest::OpenApp).await
    }

    async fn ask(&self, request: SimulatorRequest) -> Result<Response> {
        self.0.call(Request::Simulator(request)).await
    }

    async fn done(&self, request: SimulatorRequest) -> Result<()> {
        self.0.done(Request::Simulator(request)).await
    }
}
