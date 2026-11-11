//! Clearing every secret when duskcode is removed.

use std::path::Path;

use anyhow::Result;

use super::Keychain;

/// Deletes every secret duskcode keeps for the config file: the vault, and the items older
/// versions made. It goes on past a failure and reports what is left, so a removal is never
/// stopped half way by a locked keychain.
pub fn forget_all(config_file: &Path) -> Result<()> {
    Keychain::erase(config_file.parent().unwrap_or(config_file))
}
