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
    /// Installs release `tag`; progress shows in the status until it is done.
    InstallUpdate {
        tag: String,
    },
    /// Closes every thread and stops the core; the last request a client sends.
    Shutdown,
}

mod catalog;
mod project;
mod settings;
mod thread;
mod voice;
mod work;

pub use catalog::CatalogRequest;
pub use project::ProjectRequest;
pub use settings::{ProviderTarget, SettingsRequest, Switch};
pub use thread::ThreadRequest;
pub use voice::VoiceRequest;
pub use work::{MAX_PICTURE_UPLOAD, WorkRequest};
