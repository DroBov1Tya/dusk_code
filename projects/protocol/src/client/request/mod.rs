//! What a client asks the core; every request gets exactly one response.

use serde::{Deserialize, Serialize};

use crate::client::Command;
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Request {
    Thread(ThreadRequest),
    Settings(SettingsRequest),
    Catalog(CatalogRequest),
    Work(WorkRequest),
    Project(ProjectRequest),
    /// A slash command that does not run in a thread.
    Command(Command),
    SystemReport,
    /// Recordings in, speech out.
    Voice(VoiceRequest),
    /// The user's own shell in the window; the agents never see it.
    Terminal(TerminalRequest),
    /// Xcode's iOS Simulator, for the desktop app on macOS.
    Simulator(SimulatorRequest),
    /// The parts plugins add to the window.
    Plugin(PluginRequest),
    /// The plugin store; it goes to the network, so it is answered beside the loop.
    Store(crate::plugin::StoreRequest),
    /// Installs release `tag`; progress shows in the status until it is done.
    InstallUpdate {
        tag: String,
    },
    /// Closes every thread and stops the core; the last request a client sends.
    Shutdown,
}

mod catalog;
mod plugin;
mod project;
mod settings;
mod simulator;
mod terminal;
mod thread;
mod voice;
mod work;

pub use catalog::CatalogRequest;
pub use plugin::PluginRequest;
pub use project::ProjectRequest;
pub use settings::{ProviderTarget, SettingsRequest, Switch};
pub use simulator::{MAX_SIM_TEXT, SimulatorRequest};
pub use terminal::{MAX_TERMINAL_INPUT, MAX_TERMINAL_SIDE, MAX_TERMINALS, TerminalRequest};
pub use thread::ThreadRequest;
pub use voice::{VoiceRequest, wav};
pub use work::{MAX_PICTURE_UPLOAD, WorkRequest};
