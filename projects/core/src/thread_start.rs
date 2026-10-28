//! Starting a thread: its workspace folder, its record and the main agent's system prompt.

use std::{
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
};

use anyhow::{Result, bail};
use uuid::Uuid;

use crate::{init::create_private_dir, paths::Paths, state::State};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartedThread {
    pub thread_id: String,
    /// Where the agent keeps temporary and downloaded files for this thread.
    pub workspace: PathBuf,
}

/// The folder is created first, so a failed write leaves an empty folder rather than a
/// thread without one. MAIN.md is copied into the thread as its first message: later
/// edits to the file do not change threads already started.
pub async fn start_thread(state: &State, paths: &Paths, source: &str) -> Result<StartedThread> {
    let main_prompt = read_prompt(&paths.main_prompt())?;
    let thread_id = Uuid::now_v7().to_string();
    let workspace = paths.thread_workspace(&thread_id)?;
    create_private_dir(&workspace)?;
    state.create_thread(&thread_id, source).await?;
    if let Some(prompt) = main_prompt {
        state.add_system_message(&thread_id, &prompt).await?;
    }
    Ok(StartedThread {
        thread_id,
        workspace,
    })
}

/// Reads a prompt file such as MAIN.md; a missing or blank file means no prompt.
pub fn read_prompt(path: &Path) -> Result<Option<String>> {
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let prompt = match fs::read_to_string(path) {
        Ok(prompt) => prompt,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(_) => bail!("cannot read {name} as UTF-8 text"),
    };
    Ok(Some(prompt).filter(|prompt| !prompt.trim().is_empty()))
}
