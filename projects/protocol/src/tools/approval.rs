//! What a tool asks a yes for.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::tools::WindowAccess;
/// What a tool asks to do.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Request {
    /// A path outside the working directory and the workspace.
    Path { path: PathBuf, write: bool },
    /// A shell command line.
    Command { command: String },
    /// Seeing the screen and independent accessibility activation. This never grants
    /// control of the user's system pointer.
    Screen { action: String },
    /// Moving and clicking/scrolling with the shared system pointer. This needs its
    /// own session grant, separate from screen access and independent activation.
    SystemPointer { action: String },
    /// Window metadata, capture or activation. None is a request to end confinement.
    Window {
        grant: Option<WindowAccess>,
        action: String,
    },
    /// Typing or pressing keys, which can do anything a shell can: asked every time,
    /// with what it sends.
    Keyboard { action: String },
}

impl Request {
    /// What `who` asks for, in a line, for any client to show.
    pub fn describe(&self, who: &str) -> String {
        match self {
            Self::Path { path, write: true } => format!("{who} wants to write {}", path.display()),
            Self::Path { path, write: false } => format!("{who} wants to read {}", path.display()),
            Self::Command { command } => format!("{who} wants to run: {command}"),
            Self::Screen { action } => format!("{who} wants to use the screen: {action}"),
            Self::SystemPointer { action } => {
                format!("{who} wants to control your system pointer: {action}")
            }
            Self::Keyboard { action } => format!("{who} wants to use the keyboard: {action}"),
            Self::Window { action, .. } => {
                format!("{who} wants to use window tools: {action}")
            }
        }
    }

    pub fn what(&self) -> String {
        match self {
            Self::Path { path, write: true } => format!("writing {}", path.display()),
            Self::Path { path, write: false } => format!("reading {}", path.display()),
            Self::Command { command } => format!("running `{command}`"),
            Self::Screen { action } => format!("using the screen: {action}"),
            Self::SystemPointer { action } => format!("controlling the system pointer: {action}"),
            Self::Keyboard { action } => format!("using the keyboard: {action}"),
            Self::Window { action, .. } => format!("using window tools: {action}"),
        }
    }
}

/// A request as the client sees it; `id` is what its answer names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApprovalRequest {
    pub id: u64,
    pub request: Request,
    /// What a yes for the session covers, as the core decides it: the folder of the
    /// path, or the program of a plain command. `None` when it cannot be offered.
    pub session: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Decision {
    Once,
    /// Also yes to the same folder or program until the core stops.
    Session,
    Deny,
}
