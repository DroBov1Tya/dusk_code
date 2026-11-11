//! Releases: what is newest, and installing it.

use crate::release::update::{self, Channel, InstallProgress, UpdateInfo};
use crate::runtime::Runtime;
use anyhow::Result;
use std::path::PathBuf;
use tracing::info;

impl Runtime {
    /// Shipped prompts, skills and agents this start brought to the build's version, by
    /// their path in the config folder.
    pub fn updated_builtin(&self) -> &[String] {
        &self.updated_builtin
    }

    /// The newest version on the update channel; `channel` switches the channel first.
    pub async fn check_update(&self, channel: Option<Channel>) -> Result<UpdateInfo> {
        if let Some(channel) = channel {
            self.config()
                .update(|config| config.set_update_channel(channel))?;
        }
        update::check_update(self.config().current().update_channel()).await
    }

    /// Installs a release while publishing the latest stage and downloaded bytes.
    pub async fn install_update_with_progress(
        &self,
        tag: &str,
        progress: tokio::sync::watch::Sender<InstallProgress>,
    ) -> Result<PathBuf> {
        let binary = update::install_with_progress(tag, progress).await?;
        info!("updated to {tag} at {}", binary.display());
        Ok(binary)
    }

    /// A newer release on the update channel whose build is ready, once the background
    /// check has seen one.
    pub fn available_update(&self) -> Option<String> {
        self.updates.found()
    }
}
