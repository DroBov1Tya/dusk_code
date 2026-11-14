//! The iOS Simulator of Xcode on a Mac: what is installed, which devices there are and
//! what one of them shows.

use serde::{Deserialize, Serialize};

/// What the core found of Xcode's simulator, in the order the setup goes.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Simulator {
    /// Xcode's tools answer; without them nothing else can be found.
    pub xcode: bool,
    /// The installed iOS runtimes by name, such as "iOS 26.2".
    pub runtimes: Vec<String>,
    /// A runtime is being downloaded.
    pub installing: bool,
    /// Why the last download failed, until another one starts.
    pub install_failed: Option<String>,
    /// The iOS devices that can boot, booted ones first.
    pub devices: Vec<SimDevice>,
    /// AXe is installed, so touches, typing and buttons reach a device.
    pub input: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SimDevice {
    pub udid: String,
    pub name: String,
    /// The runtime's name, such as "iOS 26.2".
    pub runtime: String,
    pub booted: bool,
}

/// A device's screen at one moment.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SimScreen {
    pub jpeg: Vec<u8>,
    /// The screen's size in points, the unit touches are given in.
    pub width: f32,
    pub height: f32,
    /// Counts the frames of the device's stream; ask with it for the next one.
    pub seq: u64,
}

/// The hardware buttons a device has.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SimButton {
    Home,
    Lock,
    SideButton,
    Siri,
}

impl SimButton {
    /// The name AXe knows the button by.
    pub fn name(self) -> &'static str {
        match self {
            Self::Home => "home",
            Self::Lock => "lock",
            Self::SideButton => "side-button",
            Self::Siri => "siri",
        }
    }
}
