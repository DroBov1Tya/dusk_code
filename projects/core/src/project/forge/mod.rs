//! A project's pull requests and CI. A plugin that serves the remote's host answers
//! through the forge contract, while `plugin_contracts` is on; otherwise github.com is
//! asked through the GitHub CLI (`gh`) the user installed and signed in to. The core
//! holds no token. Every answer is untrusted, parsed into fixed shapes, its text made
//! plain and capped, its pulls only the remote's own. The repository status puts them
//! beside the work tree, and the tracker keeps it fresh.

mod checked;
mod gh;
mod lookup;
mod pulls;
mod remote;
mod status;
mod watch;

#[cfg(test)]
mod tests;

use std::{collections::BTreeMap, path::Path, path::PathBuf, sync::Arc};

use dusk_protocol::plugin::contract::{ContractError, ErrorKind};
use serde::Serialize;

use crate::plugins::contract::{Contracts, Served};

pub use lookup::{Look, read as read_pull};
pub use pulls::create;
pub use remote::Remote;
pub use status::read as read_status;
pub use watch::{PERIOD as TRACK_PERIOD, Tracker};

/// What a read needs to ask a forge plugin, taken from the config and the saved projects
/// at the time of the read.
#[derive(Default)]
pub struct Forges {
    pub contracts: Arc<Contracts>,
    /// The switch `plugin_contracts`.
    pub on: bool,
    /// The plugin the user chose for a host.
    pub chosen: BTreeMap<String, String>,
    /// Every other saved project's folder, hidden from the call.
    pub others: Vec<PathBuf>,
}

impl Forges {
    /// The plugin that serves `host`, while the switch is on.
    pub fn route(&self, host: &str) -> Option<Arc<Served>> {
        if !self.on {
            return None;
        }
        let chosen = self.chosen.get(host).map(String::as_str);
        self.contracts.route("forge", host, chosen)
    }

    /// Asks `served` for `op` about the project in `dir`.
    async fn call(
        &self,
        served: &Served,
        op: &str,
        dir: &Path,
        args: impl Serialize,
    ) -> Result<serde_json::Value, ContractError> {
        let args = serde_json::to_value(args).map_err(|error| ContractError {
            kind: ErrorKind::Failed,
            message: format!("cannot write the arguments: {error}"),
            hint: String::new(),
        })?;
        self.contracts
            .call(served, "forge", op, dir, &self.others, args)
            .await
    }
}
