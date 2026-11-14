//! The iOS Simulator of Xcode on a Mac: what is installed and which devices there are.

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
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SimDevice {
    pub udid: String,
    pub name: String,
    /// The runtime's name, such as "iOS 26.2".
    pub runtime: String,
    pub booted: bool,
}
