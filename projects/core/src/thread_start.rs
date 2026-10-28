//! Starting a thread: its workspace folder, its record and the main agent's system prompt.

use std::{
    fs::File,
    io::{ErrorKind, Read},
    path::{Path, PathBuf},
};

use anyhow::{Result, anyhow, ensure};
use uuid::Uuid;

use crate::{init::create_private_dir, paths::Paths, state::State};

const MAX_MAIN_PROMPT_BYTES: u64 = 256 * 1024;

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
    let main_prompt = read_main_prompt(&paths.main_prompt())?;
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

/// A missing or blank file means no system prompt.
fn read_main_prompt(path: &Path) -> Result<Option<String>> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(anyhow!("cannot open MAIN.md")),
    };
    let mut prompt = String::new();
    file.take(MAX_MAIN_PROMPT_BYTES + 1)
        .read_to_string(&mut prompt)
        .map_err(|_| anyhow!("MAIN.md is not readable UTF-8 text"))?;
    ensure!(
        prompt.len() as u64 <= MAX_MAIN_PROMPT_BYTES,
        "MAIN.md is too large"
    );
    Ok(Some(prompt).filter(|prompt| !prompt.trim().is_empty()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        state::MessageRole,
        testing::{temp_paths, temp_state},
    };

    #[tokio::test]
    async fn main_prompt_becomes_the_first_message() {
        let paths = temp_paths("prompt");
        let state = temp_state("prompt").await;
        std::fs::create_dir_all(paths.config_root()).unwrap();
        std::fs::write(paths.main_prompt(), "Be brief.").unwrap();

        let thread = start_thread(&state, &paths, "test").await.unwrap();
        std::fs::write(paths.main_prompt(), "Changed later.").unwrap();

        let messages = state.list_messages(&thread.thread_id).await.unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].role, MessageRole::System);
        assert_eq!(messages[0].content, "Be brief.");
        state.close().await;
    }

    #[test]
    fn blank_or_missing_main_prompt_adds_nothing() {
        let paths = temp_paths("no_prompt");
        assert_eq!(read_main_prompt(&paths.main_prompt()).unwrap(), None);
        std::fs::create_dir_all(paths.config_root()).unwrap();
        std::fs::write(paths.main_prompt(), " \n").unwrap();
        assert_eq!(read_main_prompt(&paths.main_prompt()).unwrap(), None);
    }

    #[tokio::test]
    async fn creates_workspace_and_thread() {
        let paths = temp_paths("thread");
        let state = temp_state("thread").await;

        let thread = start_thread(&state, &paths, "test").await.unwrap();

        assert!(thread.workspace.is_dir());
        assert_eq!(thread.workspace.parent(), Some(paths.workspace_root()));
        let stored = state.load_thread(&thread.thread_id).await.unwrap();
        assert_eq!(stored.unwrap().source, "test");
        state.close().await;
    }
}
