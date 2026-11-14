//! Xcode's iOS Simulator, for the desktop app on macOS.

use super::{Client, unexpected};
use anyhow::Result;
use dusk_protocol::{
    client::{Request, Response, SimulatorRequest},
    system::Simulator as Found,
};

pub struct Simulator<'a>(pub(super) &'a Client);

impl Simulator<'_> {
    /// What is installed and which devices there are.
    pub async fn look(&self) -> Result<Found> {
        match self
            .0
            .call(Request::Simulator(SimulatorRequest::Look))
            .await?
        {
            Response::Simulator(found) => Ok(found),
            other => Err(unexpected(other)),
        }
    }

    /// Starts downloading the iOS runtime; `look` tells when it is done.
    pub async fn install_runtime(&self) -> Result<()> {
        self.0
            .done(Request::Simulator(SimulatorRequest::InstallRuntime))
            .await
    }

    pub async fn boot(&self, udid: String) -> Result<()> {
        self.0
            .done(Request::Simulator(SimulatorRequest::Boot { udid }))
            .await
    }

    pub async fn shutdown(&self, udid: String) -> Result<()> {
        self.0
            .done(Request::Simulator(SimulatorRequest::Shutdown { udid }))
            .await
    }

    /// The device's screen as a JPEG.
    pub async fn screen(&self, udid: String) -> Result<Vec<u8>> {
        match self
            .0
            .call(Request::Simulator(SimulatorRequest::Screen { udid }))
            .await?
        {
            Response::Picture(jpeg) => Ok(jpeg),
            other => Err(unexpected(other)),
        }
    }

    pub async fn open_app(&self) -> Result<()> {
        self.0
            .done(Request::Simulator(SimulatorRequest::OpenApp))
            .await
    }
}
