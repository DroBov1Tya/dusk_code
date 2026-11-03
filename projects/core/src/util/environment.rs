//! What the machine offers the tools, found once when the core starts: the system, and
//! whether a graphical session is there to see and drive. Built-in tools that need what
//! is missing are neither offered to the model nor switchable.

use std::process::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    MacOs,
    Linux,
    Other,
}

/// Assumes nothing: no screen until one is found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Environment {
    pub os: Os,
    /// A screen the user sits at: an Aqua session on macOS, an X11 or Wayland display
    /// on Linux. Not over SSH, in a container or on a server without one.
    pub gui: bool,
}

/// What a tool needs from the machine beyond a shell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Needs {
    Nothing,
    Gui,
}

impl Default for Environment {
    fn default() -> Self {
        Self {
            os: Os::Other,
            gui: false,
        }
    }
}

impl Environment {
    pub fn detect() -> Self {
        let os = if cfg!(target_os = "macos") {
            Os::MacOs
        } else if cfg!(target_os = "linux") {
            Os::Linux
        } else {
            Os::Other
        };
        let gui = match os {
            Os::MacOs => macos_session_is_graphical(),
            Os::Linux => ["DISPLAY", "WAYLAND_DISPLAY"]
                .iter()
                .any(|name| std::env::var_os(name).is_some_and(|value| !value.is_empty())),
            Os::Other => false,
        };
        Self { os, gui }
    }

    pub fn provides(self, needs: Needs) -> bool {
        match needs {
            Needs::Nothing => true,
            Needs::Gui => self.gui,
        }
    }
}

/// A login at the screen runs in the Aqua session; one over SSH runs in Background.
fn macos_session_is_graphical() -> bool {
    Command::new("launchctl")
        .arg("managername")
        .output()
        .is_ok_and(|output| String::from_utf8_lossy(&output.stdout).trim() == "Aqua")
}
