//! What a plugin runs on: the systems its programs are built for and the clients that
//! can draw its parts. Empty lists mean all of them.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Os {
    Macos,
    Linux,
    Windows,
}

impl Os {
    pub fn name(self) -> &'static str {
        match self {
            Self::Macos => "macos",
            Self::Linux => "linux",
            Self::Windows => "windows",
        }
    }

    pub fn current() -> Option<Self> {
        match std::env::consts::OS {
            "macos" => Some(Self::Macos),
            "linux" => Some(Self::Linux),
            "windows" => Some(Self::Windows),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Client {
    /// The desktop window.
    Gui,
    /// The terminal.
    Tui,
    /// The browser.
    Web,
}

impl Client {
    pub fn name(self) -> &'static str {
        match self {
            Self::Gui => "gui",
            Self::Tui => "tui",
            Self::Web => "web",
        }
    }
}

/// Whether `list` lets `item` in; an empty list lets everything in.
pub fn allows<T: PartialEq>(list: &[T], item: &T) -> bool {
    list.is_empty() || list.contains(item)
}

/// Whether this system is one `platforms` lets in.
pub fn runs_here(platforms: &[Os]) -> bool {
    platforms.is_empty() || Os::current().is_some_and(|os| platforms.contains(&os))
}
