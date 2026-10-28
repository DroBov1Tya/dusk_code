//! Starting a chat session: its workspace folder, its thread record and the main
//! agent's system prompt.

use std::{
    fs::File,
    io::{ErrorKind, Read},
    path::{Path, PathBuf},
};

use anyhow::{Result, anyhow, ensure};
use uuid::Uuid;

use crate::{
    init::create_private_dir,
    paths::Paths,
    state::{ApprovalMode, MessageInput, MessageRole, State, ThreadInput},
};

const MAX_MAIN_PROMPT_BYTES: u64 = 256 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    pub thread_id: String,
    /// Where the agent keeps temporary and downloaded files for this session.
    pub workspace: PathBuf,
}

/// The folder is created first, so a failed thread write leaves an empty folder
/// rather than a thread without one. MAIN.md is copied into the thread as its first
/// message: later edits to the file do not change chats already started.
pub async fn start_session(state: &State, paths: &Paths, source: &str) -> Result<Session> {
    let main_prompt = read_main_prompt(&paths.main_prompt())?;
    let thread_id = Uuid::now_v7().to_string();
    let workspace = paths.session_workspace(&thread_id)?;
    create_private_dir(&workspace)?;
    state.create_thread(&thread_id, new_thread(source)).await?;
    if let Some(prompt) = main_prompt {
        let message = system_message(&thread_id, &workspace, prompt);
        state
            .append_message(&format!("main-prompt:{thread_id}"), message)
            .await?;
    }
    Ok(Session {
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

fn new_thread(source: &str) -> ThreadInput {
    ThreadInput {
        source: source.to_owned(),
        tokens_used: 0,
        compaction_count: 0,
        sandbox_policy: "none".to_owned(),
        approval_mode: ApprovalMode::Never,
        git_branch: None,
        git_commit_sha: None,
        git_origin_url: None,
        title: String::new(),
    }
}

fn system_message(thread_id: &str, workspace: &Path, prompt: String) -> MessageInput {
    MessageInput {
        thread_id: thread_id.to_owned(),
        job_id: None,
        role: MessageRole::System,
        agent_id: None,
        content: prompt,
        reasoning: None,
        cwd: workspace.to_string_lossy().into_owned(),
        attachments: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{temp_paths, temp_state};

    #[tokio::test]
    async fn main_prompt_becomes_the_first_message() {
        let paths = temp_paths("prompt");
        let state = temp_state("prompt").await;
        std::fs::create_dir_all(paths.config_root()).unwrap();
        std::fs::write(paths.main_prompt(), "Be brief.").unwrap();

        let session = start_session(&state, &paths, "test").await.unwrap();
        std::fs::write(paths.main_prompt(), "Changed later.").unwrap();

        let messages = state
            .list_messages(&session.thread_id, 0, 10)
            .await
            .unwrap();
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
        let paths = temp_paths("session");
        let state = temp_state("session").await;

        let session = start_session(&state, &paths, "test").await.unwrap();

        assert!(session.workspace.is_dir());
        assert_eq!(session.workspace.parent(), Some(paths.workspace_root()));
        let thread = state.load_thread(&session.thread_id).await.unwrap();
        assert_eq!(thread.unwrap().source, "test");
        state.close().await;
    }
}
