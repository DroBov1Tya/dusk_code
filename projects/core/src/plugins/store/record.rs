//! What the store did on this machine, in `store.json` of the config folder, where no
//! plugin's program can write: the newest list it accepted, so an older signed list
//! cannot roll it back, and the plugins it installed, so it never takes a folder the
//! user made or copied for one of its own.

use std::{collections::BTreeMap, path::PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::init::paths::Paths;

const FILE: &str = "store.json";
const MAX_BYTES: u64 = 1024 * 1024;

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Record {
    /// The highest `serial` of a list accepted here.
    pub serial: u64,
    pub installed: BTreeMap<String, Installed>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Installed {
    pub version: String,
    pub sha256: String,
}

impl Record {
    /// The record, or an empty one when there is none yet. A damaged record reads as an
    /// error rather than as empty, which would forget the serial.
    pub fn load(paths: &Paths) -> Result<Self> {
        let file = file(paths);
        if !file.exists() {
            return Ok(Self::default());
        }
        let bytes = crate::init::files::read_regular(&file, MAX_BYTES)?;
        serde_json::from_slice(&bytes).context("the store's record is damaged")
    }

    pub fn save(&self, paths: &Paths) -> Result<()> {
        crate::init::files::write_private(&file(paths), &serde_json::to_vec_pretty(self)?)
    }

    /// The version installed from the store, when its folder is still there.
    pub fn version_of(&self, paths: &Paths, name: &str) -> Option<String> {
        let folder = super::super::catalog::folder(paths).join(name);
        folder
            .is_dir()
            .then(|| self.installed.get(name).map(|item| item.version.clone()))
            .flatten()
    }
}

fn file(paths: &Paths) -> PathBuf {
    paths.config_root().join(FILE)
}
