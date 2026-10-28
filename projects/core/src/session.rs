//! Starting a chat session: its workspace folder and its thread record.

use std::path::PathBuf;

use anyhow::Result;
use uuid::Uuid;

use crate::{
    init::create_private_dir,
    paths::Paths,
    state::{ApprovalMode, State, ThreadInput},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    pub thread_id: String,
    /// Where the agent keeps temporary and downloaded files for this session.
    pub workspace: PathBuf,
}

/// The folder is created first, so a failed thread write leaves an empty folder
/// rather than a thread without one.
pub async fn start_session(state: &State, paths: &Paths, source: &str) -> Result<Session> {
    let thread_id = Uuid::now_v7().to_string();
    let workspace = paths.session_workspace(&thread_id)?;
    create_private_dir(&workspace)?;
    state.create_thread(&thread_id, new_thread(source)).await?;
    Ok(Session {
        thread_id,
        workspace,
    })
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{temp_paths, temp_state};

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
