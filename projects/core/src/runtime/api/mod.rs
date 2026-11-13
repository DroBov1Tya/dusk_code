//! The runtime's calls, one file for each area a client asks about. Each file is an `impl Runtime`
//! block: the struct and its start are in `runtime.rs`.

mod agents;
mod chatgpt;
mod context;
mod mcp;
mod model;
mod plugins;
mod providers;
mod secrets;
mod settings;
mod skills;
mod system;
mod threads;
mod updates;
mod voice;
mod work;

pub(super) use chatgpt::keep_renewed as keep_renewed_chatgpt;

use super::{Launcher, Runtime};

impl Runtime {
    /// The launcher that holds `thread_id`: the temporary one for a temporary chat and
    /// its agents, the stored one for everything else.
    pub(super) async fn of(&self, thread_id: &str) -> &Launcher {
        match self.temporary.state.load_thread(thread_id).await {
            Ok(Some(_)) => &self.temporary,
            _ => &self.launcher,
        }
    }
}
