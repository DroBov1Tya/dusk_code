//! Where a plugin's part shows in the window and what a click on it opens.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Place {
    /// An icon in the header's tools.
    Header,
    /// An icon in the rail of places.
    Rail,
    /// An icon beside the message box.
    Composer,
    /// A page under Settings.
    Settings,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Opens {
    /// A tab of the dock at the right of the work.
    RightPanel,
    /// A tab of the dock under the work.
    BottomPanel,
    /// A window over the page.
    Window,
}

/// One part of a plugin, as the core found it in the plugin's manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Part {
    pub plugin: String,
    /// Unique within the plugin.
    pub id: String,
    pub title: String,
    /// The icon as SVG text, checked by the core: small and with nothing it loads.
    pub icon: Option<String>,
    pub place: Place,
    pub opens: Opens,
    /// The platforms it is offered on, such as `macos`; all when empty.
    pub platforms: Vec<String>,
}
