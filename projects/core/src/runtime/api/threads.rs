//! Opening, listing and forking threads.

use crate::runtime::agent::Input;
use crate::runtime::{Runtime, ThreadHandle};
use crate::{
    runtime::thread::start::{FORK_INTRO, project_dir, start_fork, start_thread},
    state::{LocalUnsent, Message, State, Task, Thread},
};
use anyhow::{Result, ensure};
use dusk_protocol::thread::{Proposal, ProposalState};
use std::{collections::HashSet, path::Path};
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

    /// Brings the graph of the project in `dir` up to date in the background, after
    /// something outside a turn changed its files: a git operation the core served.
    pub fn refresh_graph(&self, dir: &Path) {
        if self.launcher.config.current().limits().project_graph {
            crate::project::graph::refresh_soon(
                std::sync::Arc::clone(&self.launcher.state),
                dir.to_owned(),
                Vec::new(),
            );
        }
    }

    /// The stored threads' database, then the one of this run's temporary chats.
    pub fn states(&self) -> [&State; 2] {
        [&self.launcher.state, &self.temporary.state]
    }

    /// The database that holds `thread_id`, and where its files are.
    pub async fn state_of(&self, thread_id: &str) -> &State {
        &self.of(thread_id).await.state
    }

    pub async fn places_of(&self, thread_id: &str) -> (&State, &crate::init::paths::Paths) {
        let launcher = self.of(thread_id).await;
        (&launcher.state, &launcher.paths)
    }

    /// The stored threads, and before them the temporary chats of this run.
    pub async fn list_threads(&self) -> Result<Vec<Thread>> {
        let mut threads = self.temporary.state.list_threads().await?;
        for thread in &mut threads {
            thread.temporary = true;
        }
        threads.extend(self.launcher.state.list_threads().await?);
        Ok(threads)
    }

    /// A chat that is never written to disk: it lives in memory, its files in the
    /// system's temporary folder, and it writes no memory. Gone when the core stops.
    pub async fn new_temporary_thread(&self, source: &str, cwd: &Path) -> Result<ThreadHandle> {
        self.temporary.new_thread(source, cwd).await
    }

    /// The whole history of a thread, compacted messages included, oldest first.
    pub async fn thread_messages(&self, thread_id: &str) -> Result<Vec<Message>> {
        let launcher = self.of(thread_id).await;
        launcher.state.list_messages(thread_id).await
    }

    /// The thread's pins, oldest message first.
    pub async fn pins(&self, thread_id: &str) -> Result<Vec<dusk_protocol::thread::Pin>> {
        let launcher = self.of(thread_id).await;
        launcher.state.pins(thread_id).await
    }

    /// Pins or unpins one of the thread's messages; answers with its pins.
    pub async fn set_pinned(
        &self,
        thread_id: &str,
        message_id: &str,
        pinned: bool,
    ) -> Result<Vec<dusk_protocol::thread::Pin>> {
        let launcher = self.of(thread_id).await;
        launcher
            .state
            .set_pinned(thread_id, message_id, pinned)
            .await
    }

    /// What was said in any chat that contains `text`, best matches first.
    /// Only in the chats of `within`, when it is given.
    pub async fn search_chats(
        &self,
        text: &str,
        within: Option<&HashSet<String>>,
    ) -> Result<Vec<dusk_protocol::thread::Found>> {
        self.launcher.state.search_chats(text, 40, within).await
    }

    /// Why the thread's newest turn failed, for a client that joins it to show the row the
    /// others showed; none when it did not fail.
    pub async fn last_failure(
        &self,
        thread_id: &str,
    ) -> Result<Option<dusk_protocol::thread::TurnFailure>> {
        let launcher = self.of(thread_id).await;
        launcher.state.last_failure(thread_id).await
    }

    /// The chat's goal, for a client that joins it to show what the others show.
    pub async fn goal(&self, thread_id: &str) -> Result<Option<dusk_protocol::thread::Goal>> {
        let launcher = self.of(thread_id).await;
        launcher.state.goal(thread_id).await
    }

    /// An agent below the chat works, so a client joining it shows it at work.
    pub async fn agents_at_work(&self, thread_id: &str) -> Result<bool> {
        let launcher = self.of(thread_id).await;
        launcher
            .mailboxes
            .agents_at_work(&launcher.state, thread_id)
            .await
    }

    /// Local messages waiting for an explicit resend. They are not model history.
    pub async fn local_unsent(&self, thread_id: &str) -> Result<Vec<LocalUnsent>> {
        let launcher = self.of(thread_id).await;
        launcher.state.local_unsent(thread_id).await
    }

    /// Explicitly queue one local message. Never called during startup or recovery.
    pub async fn resend_local_unsent(&self, thread_id: &str, id: &str) -> Result<()> {
        let launcher = self.of(thread_id).await;
        ensure!(self.model().is_some(), "provider is unavailable");
        launcher.state.promote_local_unsent(thread_id, id).await?;
        if let Some(mailbox) = launcher.mailboxes.get(thread_id).await {
            mailbox.send(Input::Sent);
        }
        Ok(())
    }

    /// The task board an agent keeps in a thread.
    pub async fn thread_tasks(&self, thread_id: &str) -> Result<Vec<Task>> {
        let launcher = self.of(thread_id).await;
        launcher.state.thread_tasks(thread_id).await
    }

    /// Starts a thread whose tools work in `cwd`, and the task that runs it. The task
    /// ends once the client drops every `ThreadInput` and the queue is done.
    pub async fn new_thread(&self, source: &str, cwd: &Path) -> Result<ThreadHandle> {
        self.launcher.new_thread(source, cwd).await
    }

    /// Makes the session the agent of `thread_id` proposed as `proposal_id`: a new chat
    /// in the proposal's project (the chat's own when none) and mode, for the caller to
    /// open and send the brief to. The proposal stays waiting until the caller stores the
    /// outcome with `proposal_started` once the brief is sent, holding the lock returned
    /// until then, so two clicks cannot both start it and a brief that fails to send
    /// leaves the card to start again. Refused, with nothing made, when the proposal was
    /// answered already or its mode is gone.
    pub async fn start_proposal(
        &self,
        source: &str,
        thread_id: &str,
        proposal_id: &str,
    ) -> Result<(ThreadHandle, Proposal, tokio::sync::MutexGuard<'static, ()>)> {
        use crate::tools::propose_session;
        // One answer at a time, so two clicks cannot both find it waiting.
        let answering = propose_session::ANSWERING.lock().await;
        let launcher = self.of(thread_id).await;
        let state = &launcher.state;
        let (proposal, settled) = propose_session::find(state, thread_id, proposal_id).await?;
        anyhow::ensure!(
            settled == ProposalState::Pending,
            "this proposal was answered already"
        );
        let cwd = match &proposal.project {
            Some(dir) => std::path::PathBuf::from(dir),
            None => {
                let chat = state.load_thread(thread_id).await?;
                chat.and_then(|chat| chat.project)
                    .map_or_else(|| self.paths().workspace_root().to_path_buf(), Into::into)
            }
        };
        let mode = match &proposal.mode {
            Some(mode) => {
                let known = crate::catalog::modes::Modes::new(&self.paths().modes_dir()).list();
                anyhow::ensure!(
                    known.contains(mode),
                    "the mode {mode} of this proposal no longer exists"
                );
                mode.clone()
            }
            None => self.config().current().mode().to_owned(),
        };
        let handle = launcher.new_thread_in(source, &cwd, &mode, None).await?;
        Ok((handle, proposal, answering))
    }

    /// Stores that the proposal `proposal` of `thread_id` started as the chat `started`,
    /// once its brief was sent; the caller still holds `start_proposal`'s lock.
    pub async fn proposal_started(
        &self,
        thread_id: &str,
        proposal: &Proposal,
        started: &str,
    ) -> Result<bool> {
        let state = ProposalState::Started {
            thread_id: started.to_owned(),
        };
        let store = &self.of(thread_id).await.state;
        crate::tools::propose_session::settle(
            store,
            thread_id,
            &proposal.id,
            &proposal.title,
            &state,
        )
        .await
    }

    /// Stores how the user answered the proposal `proposal_id`, for the chat's next turn;
    /// false when it was answered already.
    pub async fn settle_proposal(
        &self,
        thread_id: &str,
        proposal_id: &str,
        state: &ProposalState,
    ) -> Result<bool> {
        use crate::tools::propose_session;
        let _answering = propose_session::ANSWERING.lock().await;
        let store = &self.of(thread_id).await.state;
        let (proposal, settled) = propose_session::find(store, thread_id, proposal_id).await?;
        if settled != ProposalState::Pending {
            return Ok(false);
        }
        propose_session::settle(store, thread_id, proposal_id, &proposal.title, state).await
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
        self.launcher.new_thread_on(source, cwd, Some(choice)).await
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

    /// A new chat that holds `from`'s history up to and including `message_id`, and the
    /// task that runs it. Nothing running in `from` comes along.
    pub async fn fork_thread_at(
        &self,
        source: &str,
        from: &str,
        message_id: &str,
        cwd: &Path,
    ) -> Result<ThreadHandle> {
        let thread = start_fork(
            &self.launcher.state,
            &self.launcher.paths,
            source,
            from,
            message_id,
        )
        .await?;
        info!("thread {} forked from {from}", thread.thread_id);
        self.launcher
            .run_thread(thread.thread_id, &thread.workspace, cwd)
            .await
    }

    /// Picks a stored thread up again, with its tools working in `cwd`. What it left
    /// unfinished is settled first: a running turn goes on or waits for the user, and
    /// messages sent before a crash are taken in.
    pub async fn resume_thread(&self, thread_id: &str, cwd: &Path) -> Result<ThreadHandle> {
        let launcher = self.of(thread_id).await;
        launcher.resume_thread(thread_id, cwd).await
    }
}
