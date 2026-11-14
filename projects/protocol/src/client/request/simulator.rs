//! The iOS Simulator, for the user to see and use an app they build; macOS only.

use serde::{Deserialize, Serialize};

use crate::system::SimButton;

/// The most characters one `Type` carries.
pub const MAX_SIM_TEXT: usize = 4096;

/// A device is named by its UDID as the core listed it; places on its screen are in
/// points from the top left corner.
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
    /// The device's screen as `Response::SimScreen`: the first frame after `after`, or
    /// the last one when the screen holds still for a moment.
    Screen {
        udid: String,
        after: u64,
    },
    Tap {
        udid: String,
        x: u32,
        y: u32,
    },
    Swipe {
        udid: String,
        from: (u32, u32),
        to: (u32, u32),
    },
    /// Text typed on the device's keyboard.
    Type {
        udid: String,
        text: String,
    },
    Button {
        udid: String,
        button: SimButton,
    },
    /// Opens Simulator.app with its own window of the devices.
    OpenApp,
}
