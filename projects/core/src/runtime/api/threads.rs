//! Opening, listing and forking threads.

use crate::runtime::agent::Input;
use crate::runtime::{Runtime, ThreadHandle};
use crate::{
    runtime::thread::thread_start::{FORK_INTRO, project_dir, start_thread},
    state::{LocalUnsent, Message, State, Task, Thread},
};
use anyhow::{Result, ensure};
use std::path::Path;
use tracing::info;

impl Runtime {
    /// Every thread, the most recently active first, for a client to offer.
    /// The chats started in `cwd`, the most recently active first.
    pub async fn project_threads(&self, cwd: &Path) -> Result<Vec<Thread>> {
        self.launcher.state.project_threads(&project_dir(cwd)).await
    }

    pub fn state(&self) -> &State {
        &self.launcher.state
    }

    pub async fn list_threads(&self) -> Result<Vec<Thread>> {
        self.launcher.state.list_threads().await
    }

    /// The whole history of a thread, compacted messages included, oldest first.
    pub async fn thread_messages(&self, thread_id: &str) -> Result<Vec<Message>> {
        self.launcher.state.list_messages(thread_id).await
    }

    /// Local messages waiting for an explicit resend. They are not model history.
    pub async fn local_unsent(&self, thread_id: &str) -> Result<Vec<LocalUnsent>> {
        self.launcher.state.local_unsent(thread_id).await
    }

    /// Explicitly queue one local message. Never called during startup or recovery.
    pub async fn resend_local_unsent(&self, thread_id: &str, id: &str) -> Result<()> {
        ensure!(self.model().is_some(), "provider is unavailable");
        self.launcher
            .state
            .promote_local_unsent(thread_id, id)
            .await?;
        if let Some(mailbox) = self.launcher.mailboxes.get(thread_id).await {
            mailbox.send(Input::Sent);
        }
        Ok(())
    }

    /// The task board an agent keeps in a thread.
    pub async fn thread_tasks(&self, thread_id: &str) -> Result<Vec<Task>> {
        self.launcher.state.thread_tasks(thread_id).await
    }

    /// Starts a thread whose tools work in `cwd`, and the task that runs it. The task
    /// ends once the client drops every `ThreadInput` and the queue is done.
    pub async fn new_thread(&self, source: &str, cwd: &Path) -> Result<ThreadHandle> {
        self.launcher.new_thread(source, cwd).await
    }

    /// A new thread that asks `model`, as `provider/model`, instead of the chosen one.
    pub async fn new_thread_with_model(
        &self,
        source: &str,
        cwd: &Path,
        model: &str,
    ) -> Result<ThreadHandle> {
        let config = self.config().current();
        let choice = crate::runtime::model_choice::Choice::pick(&config, Some(model)).await?;
        let model = std::sync::Arc::new(crate::runtime::model_choice::ModelChoice::new(choice));
        self.launcher.new_thread_on(source, cwd, Some(model)).await
    }

    /// A new chat thread that starts from a /fork summary of another, after its prompt.
    pub async fn fork_thread(
        &self,
        source: &str,
        summary: &str,
        cwd: &Path,
    ) -> Result<ThreadHandle> {
        let mode = self.config().current().mode().to_owned();
        let thread = start_thread(
            &self.launcher.state,
            &self.launcher.paths,
            source,
            &mode,
            cwd,
        )
        .await?;
        let summary = format!("{FORK_INTRO}{summary}");
        self.launcher
            .state
            .add_system_message(&thread.thread_id, &summary)
            .await?;
        info!("thread {} forked", thread.thread_id);
        self.launcher
            .run_thread(thread.thread_id, &thread.workspace, cwd)
            .await
    }

    /// Picks a stored thread up again, with its tools working in `cwd`. What it left
    /// unfinished is closed first: a running turn ends as interrupted, and messages sent
    /// before a crash are taken in.
    pub async fn resume_thread(&self, thread_id: &str, cwd: &Path) -> Result<ThreadHandle> {
        self.launcher.resume_thread(thread_id, cwd).await
    }
}
