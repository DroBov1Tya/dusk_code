//! Restore must run before logging, first-start initialization and opening SQLite.

use std::path::Path;

use anyhow::Result;
use dusk_core::app::{self, Paths};

pub fn run(archive_path: &Path) -> Result<()> {
    let paths = Paths::from_home()?;
    app::restore(&paths, archive_path)?;
    println!(
        "Restored config and chats to {} and {}.\nThe archive contains chat history; keep it private. Keys kept in the system keychain are not in it: enter them again.\nSaved absolute project paths are unchanged. Start duskcode normally when ready.",
        paths.config_root().display(),
        paths.workspace_root().display(),
    );
    Ok(())
}

pub fn recover() -> Result<()> {
    let paths = Paths::from_home()?;
    app::recover(&paths)?;
    println!(
        "Interrupted restore completed. Both data roots match the journaled archive. Start duskcode normally when ready."
    );
    Ok(())
}
