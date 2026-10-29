//! Starting a thread: its workspace folder, its record and its system prompt, which is
//! MAIN.md for a chat and for the general agent, and the agent's own text for others.

use std::{
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
};

use anyhow::{Result, bail};
use uuid::Uuid;

use crate::{
    init::create_private_dir,
    paths::Paths,
    state::{State, Thread},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartedThread {
    pub thread_id: String,
    /// An agent's name; `None` for a chat.
    pub name: Option<String>,
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
        name: None,
        workspace,
    })
}

/// A thread for `agent` under the thread that asks it, with `prompt` as its system prompt.
pub async fn start_agent_thread(
    state: &State,
    paths: &Paths,
    parent: &str,
    agent: &str,
    prompt: Option<String>,
) -> Result<StartedThread> {
    // The name comes from the ID, so a new ID gives a sibling's name a way out.
    let taken: Vec<String> = state
        .agent_threads(parent)
        .await?
        .into_iter()
        .map(|thread| display_name(&thread))
        .collect();
    let (thread_id, name) = loop {
        let thread_id = Uuid::now_v7().to_string();
        let name = agent_name(&thread_id);
        if !taken.contains(&name) {
            break (thread_id, name);
        }
    };
    let workspace = paths.thread_workspace(&thread_id)?;
    create_private_dir(&workspace)?;
    state
        .create_agent_thread(&thread_id, parent, agent, &name)
        .await?;
    if let Some(prompt) = prompt.filter(|prompt| !prompt.trim().is_empty()) {
        state.add_system_message(&thread_id, &prompt).await?;
    }
    Ok(StartedThread {
        thread_id,
        name: Some(name),
        workspace,
    })
}

/// The name an agent's thread goes by, made from its ID, so every client shows the same
/// one: a word and two hex digits, such as `Fable-3a`.
pub fn agent_name(thread_id: &str) -> String {
    const NAMES: [&str; 8] = [
        "Aster", "Beryl", "Cinder", "Dapple", "Ember", "Fable", "Glint", "Hollow",
    ];
    let seed = name_seed(thread_id);
    format!(
        "{}-{:02x}",
        NAMES[(seed as usize) % NAMES.len()],
        (seed >> 24) & 0xff
    )
}

/// What a thread goes by: its stored name, or for a thread older than names, the one its
/// ID gives.
pub fn display_name(thread: &Thread) -> String {
    thread
        .name
        .clone()
        .unwrap_or_else(|| agent_name(&thread.id))
}

/// FNV-1a: stable across runs and builds, unlike the standard hasher.
pub fn name_seed(thread_id: &str) -> u64 {
    thread_id
        .bytes()
        .fold(0xcbf29ce484222325_u64, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
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
