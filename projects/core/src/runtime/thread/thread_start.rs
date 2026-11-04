//! Starting a thread: its workspace folder, its record and its system prompt, which is
//! the mode's for a chat, MAIN mode's for the general agent, and the agent's own text for
//! others.

use std::{
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
};

use anyhow::{Result, bail};
use uuid::Uuid;

use crate::{
    catalog::modes::Modes,
    init::create_private_dir,
    init::paths::Paths,
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

/// Stands for a chat's prompt when its mode has none; never sent to the model.
pub const NO_PROMPT: &str = "(no prompt)";
/// Introduces a /fork summary to the model in the new thread; it also tells a summary
/// from a prompt among the system messages.
pub const FORK_INTRO: &str = "This thread goes on from an earlier one, described below. Its \
messages are not here; work from this description.\n\n";

/// Where an agent's folder sits inside its chat's.
const AGENTS_FOLDER: &str = "agents";

/// Where a thread's files are: the folder it was given, or, for a thread from before
/// folders were named by date, the one named by its ID.
pub async fn workspace(state: &State, paths: &Paths, thread_id: &str) -> Result<PathBuf> {
    match state.thread_folder(thread_id).await? {
        Some(folder) => paths.workspace_folder(&folder),
        None => paths.thread_workspace(thread_id),
    }
}

/// A new chat's folder, named by the day for people to find it: `03_11_2026`, then
/// `03_11_2026-1` and on for later chats that day. Made here, so two chats starting at
/// once cannot take the same name.
fn new_chat_folder(paths: &Paths) -> Result<(String, PathBuf)> {
    create_private_dir(paths.workspace_root())?;
    let day = chrono::Local::now().format("%d_%m_%Y").to_string();
    for number in 0..10_000 {
        let folder = match number {
            0 => day.clone(),
            number => format!("{day}-{number}"),
        };
        let path = paths.workspace_folder(&folder)?;
        let mut builder = fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        match builder.create(&path) {
            Ok(()) => return Ok((folder, path)),
            Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
            Err(_) => bail!("cannot create the chat's folder"),
        }
    }
    bail!("too many chats today for a folder of their own")
}

/// `cwd` as the one name a folder goes by, however it was reached.
pub fn project_dir(cwd: &Path) -> String {
    cwd.canonicalize()
        .unwrap_or_else(|_| cwd.to_owned())
        .to_string_lossy()
        .into_owned()
}

/// The folder is created first, so a failed write leaves an empty folder rather than a
/// thread without one. A chat's first message is the prompt of the mode it starts in.
pub async fn start_thread(
    state: &State,
    paths: &Paths,
    source: &str,
    mode: &str,
    cwd: &Path,
) -> Result<StartedThread> {
    let prompt = Modes::new(&paths.modes_dir()).prompt(mode)?;
    let thread_id = Uuid::now_v7().to_string();
    let (folder, workspace) = new_chat_folder(paths)?;
    state.create_thread(&thread_id, source).await?;
    state.set_thread_folder(&thread_id, &folder).await?;
    state
        .set_thread_project(&thread_id, &project_dir(cwd))
        .await?;
    let prompt = prompt.as_deref().unwrap_or(NO_PROMPT);
    state.add_system_message(&thread_id, prompt).await?;
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
    // Inside the chat's folder, so a chat's agents stay with it on disk.
    let root = state.root_thread(parent).await?;
    let chat = match state.thread_folder(&root).await? {
        Some(folder) => folder,
        None => root,
    };
    let folder = format!("{chat}/{AGENTS_FOLDER}/{thread_id}");
    let workspace = paths.workspace_folder(&folder)?;
    create_private_dir(&workspace)?;
    state
        .create_agent_thread(&thread_id, parent, agent, &name)
        .await?;
    state.set_thread_folder(&thread_id, &folder).await?;
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

pub fn name_seed(thread_id: &str) -> u64 {
    fnv1a(thread_id.as_bytes())
}

/// FNV-1a: stable across runs and builds, unlike the standard hasher. Not a defence
/// against anyone.
pub fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325_u64, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{temp_paths, temp_state};

    #[tokio::test]
    async fn chats_are_filed_by_day_and_agents_inside_them() {
        let (state, paths) = (temp_state("folders").await, temp_paths("folders"));
        let cwd = std::env::temp_dir();
        let first = start_thread(&state, &paths, "test", "MAIN", &cwd)
            .await
            .unwrap();
        let second = start_thread(&state, &paths, "test", "MAIN", &cwd)
            .await
            .unwrap();
        let day = chrono::Local::now().format("%d_%m_%Y").to_string();
        assert_eq!(first.workspace, paths.workspace_root().join(&day));
        assert_eq!(
            second.workspace,
            paths.workspace_root().join(format!("{day}-1"))
        );
        let agent = start_agent_thread(&state, &paths, &second.thread_id, "general", None)
            .await
            .unwrap();
        let inside = second.workspace.join(AGENTS_FOLDER).join(&agent.thread_id);
        assert_eq!(agent.workspace, inside);
        assert!(inside.is_dir());
        let found = workspace(&state, &paths, &agent.thread_id).await.unwrap();
        assert_eq!(found, inside);
        // A thread from before dated folders keeps the one named by its ID.
        state.create_thread("older", "test").await.unwrap();
        let older = workspace(&state, &paths, "older").await.unwrap();
        assert_eq!(older, paths.workspace_root().join("older"));
    }
}
