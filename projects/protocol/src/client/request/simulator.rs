//! The iOS Simulator, for the user to see an app they build; macOS only.

use serde::{Deserialize, Serialize};

/// A device is named by its UDID as the core listed it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SimulatorRequest {
    /// Answers with `Response::Simulator`.
    Look,
    /// Starts downloading the iOS runtime; `Look` tells when it is done.
    InstallRuntime,
    Boot {
        udid: String,
    },
    Shutdown {
        udid: String,
    },
    /// The device's screen now, as `Response::Picture` holding a JPEG.
    Screen {
        udid: String,
    },
    /// Opens Simulator.app, where the device takes clicks and keys.
    OpenApp,
}
