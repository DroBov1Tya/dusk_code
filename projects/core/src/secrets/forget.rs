//! Clearing every secret when duskcode is removed.

use super::{HUGGINGFACE, Keychain, provider_key};
use anyhow::{Result, ensure};
use std::path::Path;

/// Deletes every secret duskcode keeps for the config file: the Hugging Face token and the
/// key of each provider the file names. It goes on past a failure and reports the names
/// left, so a removal is never stopped half way by a locked keychain.
pub fn forget_all(config_file: &Path) -> Result<()> {
    let root = config_file.parent().unwrap_or(config_file);
    let Some(store) = Keychain::open(root) else {
        return Ok(());
    };
    let mut names = vec![HUGGINGFACE.to_owned()];
    // A config that cannot be read names no providers; their keys are then left behind.
    if let Ok(config) = crate::config::Config::load_from(config_file) {
        names.extend(config.providers().keys().map(|name| provider_key(name)));
    }
    let left: Vec<_> = names
        .into_iter()
        .filter(|name| store.delete(name).is_err())
        .collect();
    ensure!(
        left.is_empty(),
        "these stay in the keychain: {}",
        left.join(", ")
    );
    Ok(())
}
