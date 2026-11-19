//! Contracts: the core asks a plugin's contract program for one answer of a domain it
//! serves, such as a forge's pull requests. The program runs once per call, confined to
//! that call's project, so a project folder is a per-call grant, never a standing one.
//!
//! - `call`: one call, from the confinement to the result line;
//! - `forge`: a forge answer checked against the remote it is about;
//! - `untrusted`: text from a program made plain and capped.

// Until routing and the forge read through it (tasks 6 and 7 of the contract plan).
#![allow(dead_code, reason = "routing and the forge call it next")]

mod call;
pub mod forge;
pub mod untrusted;

#[cfg(test)]
mod tests;

use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use dusk_protocol::plugin::{
    Provides, Tier,
    contract::{Call, ContractError, ErrorKind},
};
use parking_lot::{Mutex, RwLock};
use tokio::sync::Semaphore;
use tracing::debug;

use super::Isolation;

/// At most this many calls of one plugin run at once; more wait for a slot.
const SLOTS: usize = 4;

/// A plugin that is on and serves contracts.
pub struct Served {
    pub plugin: String,
    pub tier: Tier,
    pub provides: Provides,
    pub command: Vec<String>,
    /// Its programs' confinement; each call narrows a copy to one project.
    pub isolation: Isolation,
    slots: Arc<Semaphore>,
}

impl Served {
    pub fn new(
        plugin: String,
        tier: Tier,
        provides: Provides,
        command: Vec<String>,
        isolation: Isolation,
    ) -> Self {
        Self {
            plugin,
            tier,
            provides,
            command,
            isolation,
            slots: Arc::new(Semaphore::new(SLOTS)),
        }
    }

    /// What changes the routing or the confinement of its calls.
    fn key(&self) -> (String, Vec<String>, String) {
        let hosts = self
            .provides
            .forge
            .as_ref()
            .map(|forge| forge.hosts.clone())
            .unwrap_or_default();
        (self.plugin.clone(), hosts, self.isolation.fingerprint())
    }
}

/// The plugins that serve contracts, and the way to call them.
#[derive(Default)]
pub struct Contracts {
    served: RwLock<Vec<Arc<Served>>>,
    revision: AtomicU64,
    /// (plugin, op) answered `unsupported` in this run; not asked again.
    unsupported: Mutex<BTreeSet<(String, String)>>,
}

impl Contracts {
    /// Replaces the plugins served. A plugin served before keeps its slots, so calls
    /// still running count against the same limit.
    pub fn set(&self, served: Vec<Served>) {
        let mut current = self.served.write();
        let old: BTreeSet<_> = current.iter().map(|served| served.key()).collect();
        let new: Vec<Arc<Served>> = served
            .into_iter()
            .map(|mut served| {
                if let Some(before) = current.iter().find(|old| old.plugin == served.plugin) {
                    served.slots = Arc::clone(&before.slots);
                }
                Arc::new(served)
            })
            .collect();
        if new
            .iter()
            .map(|served| served.key())
            .collect::<BTreeSet<_>>()
            != old
        {
            self.revision.fetch_add(1, Ordering::Relaxed);
            // An updated plugin may support what it did not.
            self.unsupported.lock().clear();
        }
        *current = new;
    }

    /// Changes whenever a plugin, the hosts it serves or its confinement changes.
    pub fn revision(&self) -> u64 {
        self.revision.load(Ordering::Relaxed)
    }

    /// Asks `served` for `op` of `contract` about the project in `folder`; `others` are
    /// the other saved projects' folders, hidden from the call. The answer's `ok` is
    /// untrusted: the caller checks it in its operation's shape.
    pub async fn call(
        &self,
        served: &Served,
        contract: &str,
        op: &str,
        folder: &Path,
        others: &[PathBuf],
        args: serde_json::Value,
    ) -> Result<serde_json::Value, ContractError> {
        let Some(version) = version(&served.provides, contract) else {
            return Err(failed(format!(
                "{} speaks no version of the contract {contract} this duskcode does",
                served.plugin
            )));
        };
        let asked = (served.plugin.clone(), op.to_owned());
        if self.unsupported.lock().contains(&asked) {
            return Err(failed(format!("{} does not support {op}", served.plugin)));
        }
        let folder = folder
            .canonicalize()
            .map_err(|error| failed(format!("cannot resolve the project's folder: {error}")))?;
        let Some(text) = folder.to_str() else {
            return Err(failed("the project's folder is not UTF-8 text".into()));
        };
        let call = Call {
            contract: contract.to_owned(),
            version,
            op: op.to_owned(),
            folder: text.to_owned(),
            args,
        };
        let answer = call::run(served, &call, &folder, others).await;
        let mut error = match answer {
            Ok(answer) => match (answer.ok, answer.error) {
                (Some(ok), None) => return Ok(ok),
                (None, Some(error)) => error,
                _ => {
                    return Err(failed(
                        "its result held neither or both of ok and error".into(),
                    ));
                }
            },
            Err(error) => error,
        };
        error.message = untrusted::line(&error.message, 200);
        let hint = untrusted::line(&error.hint, 120);
        if hint != error.hint {
            // A hint is shown as a command to type: a cut one would mislead.
            error.hint.clear();
        }
        match error.kind {
            ErrorKind::Unsupported => {
                debug!("plugin {} does not support {op}", served.plugin);
                self.unsupported.lock().insert(asked);
                error.kind = ErrorKind::Failed;
            }
            // A pull just made cannot be missing.
            ErrorKind::NotFound if op == "create_pull" => error.kind = ErrorKind::Failed,
            ErrorKind::Other => error.kind = ErrorKind::Failed,
            _ => {}
        }
        Err(error)
    }
}

/// The highest version of `contract` both the plugin and this core speak.
fn version(provides: &Provides, contract: &str) -> Option<u32> {
    use dusk_protocol::plugin::contract::forge::VERSIONS;
    let spoken = match contract {
        "forge" => &provides.forge.as_ref()?.versions,
        _ => return None,
    };
    spoken
        .iter()
        .filter(|version| VERSIONS.contains(version))
        .max()
        .copied()
}

/// A `failed` answer; `message` goes to the debug log only.
fn failed(message: String) -> ContractError {
    ContractError {
        kind: ErrorKind::Failed,
        message,
        hint: String::new(),
    }
}
