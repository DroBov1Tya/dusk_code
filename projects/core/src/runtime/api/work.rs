//! A thread's jobs, sources, plan, changes and what the user runs by hand.

use crate::runtime::Runtime;
use crate::runtime::jobs::JobInfo;
use crate::{
    catalog::mentions::Mentions,
    message::sources::ThreadSources,
    runtime::thread::thread_start::{self},
    state::{Source, UndoPoint},
    tools::Decision,
};
use anyhow::{Context, Result};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

impl Runtime {
    /// Sends a running foreground command of the thread to the background.
    pub async fn send_to_background(&self, thread_id: &str, call_id: &str) -> Result<()> {
        let launcher = self.of(thread_id).await;
        launcher.jobs.send_to_background(thread_id, call_id).await
    }

    /// Runs a command the user chose in the client, such as one from a code block, in
    /// the chat's folder and confined as its tools are. The client asked the user first;
    /// the output goes back to the client, not to the model.
    pub async fn run_for_user(&self, thread_id: &str, command: &str) -> Result<String> {
        let launcher = self.of(thread_id).await;
        let state = &launcher.state;
        let thread = state
            .load_thread(thread_id)
            .await?
            .with_context(|| format!("thread {thread_id} does not exist"))?;
        let cwd = thread
            .project
            .map(PathBuf::from)
            .context("the chat has no project folder")?;
        let workspace = thread_start::workspace(state, &launcher.paths, thread_id).await?;
        let context = crate::tools::ToolContext {
            scope: Arc::new(launcher.scope(&cwd, &workspace)?),
            config: self.config().current(),
            progress: None,
            approvals: None,
            state: Some(Arc::clone(state)),
            thread_id: thread_id.to_owned(),
        };
        crate::tools::console::run_for_user(command, &context).await
    }

    /// `message` with its long pastes, and what its `@` mentions bring, moved to the
    /// thread's sources.
    pub async fn keep_pastes(&self, thread_id: &str, message: &str) -> Result<String> {
        let launcher = self.of(thread_id).await;
        let workspace =
            thread_start::workspace(&launcher.state, &launcher.paths, thread_id).await?;
        let sources = ThreadSources {
            state: &launcher.state,
            workspace: &workspace,
            thread_id,
        };
        let kept = sources.keep_pastes(message).await?;
        sources.keep_mentions(&kept, &self.mentions()).await
    }

    pub fn mentions(&self) -> Mentions {
        Mentions::new(&self.launcher.paths.rules_root())
    }

    pub async fn sources(&self, thread_id: &str) -> Result<Vec<Source>> {
        let launcher = self.of(thread_id).await;
        launcher.state.sources(thread_id).await
    }

    /// Copies the file at `path` among the thread's sources.
    pub async fn add_source(&self, thread_id: &str, path: &Path) -> Result<Source> {
        let launcher = self.of(thread_id).await;
        let workspace =
            thread_start::workspace(&launcher.state, &launcher.paths, thread_id).await?;
        ThreadSources {
            state: &launcher.state,
            workspace: &workspace,
            thread_id,
        }
        .add_file(path)
        .await
    }

    /// Keeps a picture a client sent as bytes among the thread's sources; answers with
    /// the tag that puts it in a message.
    pub async fn add_picture(&self, thread_id: &str, name: &str, bytes: &[u8]) -> Result<String> {
        let launcher = self.of(thread_id).await;
        let workspace =
            thread_start::workspace(&launcher.state, &launcher.paths, thread_id).await?;
        let source = ThreadSources {
            state: &launcher.state,
            workspace: &workspace,
            thread_id,
        }
        .add_picture(name, bytes)
        .await?;
        Ok(dusk_protocol::thread::source_tag(&source.name))
    }

    pub async fn pin_source(&self, thread_id: &str, id: &str, pinned: bool) -> Result<()> {
        let launcher = self.of(thread_id).await;
        launcher.state.pin_source(thread_id, id, pinned).await
    }

    /// Forgets a source and deletes its file.
    pub async fn remove_source(&self, thread_id: &str, id: &str) -> Result<()> {
        let launcher = self.of(thread_id).await;
        let workspace =
            thread_start::workspace(&launcher.state, &launcher.paths, thread_id).await?;
        ThreadSources {
            state: &launcher.state,
            workspace: &workspace,
            thread_id,
        }
        .remove(id)
        .await
    }

    /// The thread's pinned plan, or none.
    pub async fn plan(&self, thread_id: &str) -> Result<Option<String>> {
        let launcher = self.of(thread_id).await;
        let state = &launcher.state;
        let workspace = thread_start::workspace(state, &launcher.paths, thread_id).await?;
        crate::tools::read_plan(state, &workspace, thread_id).await
    }

    /// Rewrites the thread's plan as the user typed it; empty text removes it.
    pub async fn save_plan(&self, thread_id: &str, text: &str) -> Result<()> {
        let launcher = self.of(thread_id).await;
        let state = &launcher.state;
        let workspace = thread_start::workspace(state, &launcher.paths, thread_id).await?;
        if text.trim().is_empty() {
            return crate::tools::clear_plan(state, &workspace, thread_id).await;
        }
        let name = crate::tools::PLAN_NAME;
        crate::tools::save_plan(state, &workspace, thread_id, name, text)
            .await
            .map(drop)
    }

    /// The git changes in the folder the thread started in.
    pub async fn chat_diff(
        &self,
        thread_id: &str,
        path: Option<&str>,
    ) -> Result<Vec<dusk_protocol::project::FileDiff>> {
        let thread = self
            .launcher
            .state
            .load_thread(thread_id)
            .await?
            .context("no such thread")?;
        let dir = thread.project.context("the thread has no folder")?;
        crate::project::diff(std::path::Path::new(&dir), path).await
    }

    /// The files the thread's commands changed in its project.
    pub async fn changed_files(
        &self,
        thread_id: &str,
    ) -> Result<Vec<dusk_protocol::project::Change>> {
        let launcher = self.of(thread_id).await;
        launcher.state.thread_files(thread_id).await
    }

    /// The background jobs of a thread, oldest first.
    pub async fn jobs(&self, thread_id: &str) -> Result<Vec<JobInfo>> {
        let launcher = self.of(thread_id).await;
        launcher.jobs.list(thread_id).await
    }

    /// What a job of the thread printed so far.
    pub async fn job_output(&self, thread_id: &str, job_id: &str) -> Result<String> {
        let launcher = self.of(thread_id).await;
        launcher.jobs.output(thread_id, job_id).await
    }

    /// Asks a running job to stop; its result still arrives like any other.
    pub async fn stop_job(&self, thread_id: &str, job_id: &str) -> Result<String> {
        let launcher = self.of(thread_id).await;
        launcher.jobs.cancel(thread_id, job_id).await
    }

    pub async fn undo_points(&self, thread_id: &str) -> Result<Vec<UndoPoint>> {
        let launcher = self.of(thread_id).await;
        launcher.state.undo_points(thread_id).await
    }

    /// False when the request is no longer waiting, such as after its turn was stopped.
    pub fn answer_approval(&self, id: u64, decision: Decision) -> bool {
        self.launcher.approvals.answer(id, decision)
            || self.temporary.approvals.answer(id, decision)
    }
}
