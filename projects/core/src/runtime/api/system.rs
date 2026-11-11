//! The machine: its platform, paths and resource use, and the archive of the data.

use crate::platform::Platform;

use crate::runtime::Runtime;

use crate::{
    init::paths::Paths,
    util::usage::{self},
};

use anyhow::Result;

pub use dusk_protocol::system::SystemReport;

use std::path::{Path, PathBuf};

impl Runtime {
    /// A SQLite snapshot plus the two standard data roots, without the backup folder.
    pub async fn migrate(&self, destination: Option<PathBuf>) -> Result<PathBuf> {
        crate::backup::archive::export(
            self.paths(),
            &self.launcher.state,
            &self.state_db,
            destination,
        )
        .await
    }

    /// The machine as the core found it at start.
    pub fn platform(&self) -> Platform {
        self.launcher.platform
    }

    /// Samples the process and measures the records; meant for a view refreshed every
    /// few seconds, not for every frame.
    pub async fn system_report(&self) -> Result<SystemReport> {
        let (threads, messages) = self.launcher.state.counts().await?;
        let wal = |suffix: &str| {
            let mut name = self.state_db.clone().into_os_string();
            name.push(suffix);
            usage::disk_bytes(Path::new(&name))
        };
        let paths = &self.launcher.paths;
        Ok(SystemReport {
            process: usage::process(),
            database_bytes: usage::disk_bytes(&self.state_db),
            wal_bytes: wal("-wal") + wal("-shm"),
            artifacts_bytes: usage::disk_bytes(&paths.artifacts_dir()),
            logs_bytes: usage::disk_bytes(&paths.logs_dir()),
            threads,
            messages,
        })
    }

    pub fn paths(&self) -> &Paths {
        &self.launcher.paths
    }
}
