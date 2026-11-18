//! What the core pushes to a client on its own.

use serde::{Deserialize, Serialize};

use crate::thread::ThreadEvent;
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Event {
    Thread {
        thread_id: String,
        event: Box<ThreadEvent>,
    },
    /// The thread's task ended; sending to it opens it again.
    Ended { thread_id: String },
    /// What a terminal printed, as raw bytes in the order they came; sent only to the
    /// client that opened it.
    Terminal { id: String, data: Vec<u8> },
    /// The terminal's shell ended, by itself or because the terminal was closed. `code`
    /// is its exit code, or none when a signal ended it.
    TerminalClosed { id: String, code: Option<i32> },
    /// The view of a plugin's part this client opened, replacing the one before.
    PluginView {
        plugin: String,
        part: String,
        root: crate::plugin::Widget,
    },
    /// A new picture for a `screen` widget of an open part, as JPEG.
    PluginFrame {
        plugin: String,
        part: String,
        widget: String,
        jpeg: Vec<u8>,
    },
    /// The repository of a saved project was changed through the core: open views of
    /// it read it again. Given to clients that may read projects.
    GitChanged { project_id: String },
    /// A project's repository status changed: its pull requests or their checks, or
    /// the work tree. Given to clients that may read projects.
    RepositoryStatus {
        project_id: String,
        status: Box<crate::project::RepositoryStatus>,
    },
    /// A plugin's short note, or why its interface stopped.
    PluginNotice { plugin: String, text: String },
}
