//! A thread's jobs, sources, plan, changes and what the user runs by hand.

use crate::runtime::Runtime;
use crate::runtime::jobs::JobInfo;
use crate::{
    catalog::mentions::Mentions,
    message::sources::{HostFiles, ThreadSources},
    runtime::ThreadEvent,
    runtime::thread::start::{self},
    state::{Source, UndoPoint},
    tools::Decision,
};
use anyhow::{Context, Result};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

/// Agents nest no deeper than this; a longer chain is cut, never followed forever.
const MAX_DEPTH: usize = 16;

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
        let workspace = start::workspace(state, &launcher.paths, thread_id).await?;
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

    /// `message` with its long pastes moved to the thread's sources and, when `files`
    /// are taken, the pictures and sounds it names by path and what its `@` mentions
    /// bring.
    pub async fn keep_pastes(
        &self,
        thread_id: &str,
        message: &str,
        files: HostFiles,
    ) -> Result<String> {
        let launcher = self.of(thread_id).await;
        let workspace = start::workspace(&launcher.state, &launcher.paths, thread_id).await?;
        let sources = ThreadSources {
            state: &launcher.state,
            workspace: &workspace,
            thread_id,
        };
        if files == HostFiles::Left {
            return sources.keep_pasted_blocks(message).await;
        }
        let sounds = crate::state::artifacts::ArtifactStore::new(launcher.paths.artifacts_dir());
        let kept = sources.keep_pastes(message, &sounds).await?;
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
        let workspace = start::workspace(&launcher.state, &launcher.paths, thread_id).await?;
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
        let workspace = start::workspace(&launcher.state, &launcher.paths, thread_id).await?;
        let source = ThreadSources {
            state: &launcher.state,
            workspace: &workspace,
            thread_id,
        }
        .add_picture(name, bytes)
        .await?;
        Ok(dusk_protocol::thread::source_tag(&source.name))
    }

    /// The bytes of the thread's picture source `name`, for a client to draw.
    pub async fn source_picture(&self, thread_id: &str, name: &str) -> Result<Vec<u8>> {
        let launcher = self.of(thread_id).await;
        let workspace = start::workspace(&launcher.state, &launcher.paths, thread_id).await?;
        ThreadSources {
            state: &launcher.state,
            workspace: &workspace,
            thread_id,
        }
        .picture(name)
        .await
    }

    /// A part of the card `card_id`, only when a message of the thread or of an agent
    /// thread below it carries that card. The id is checked before any file is read, and
    /// the path comes from the artifact folder, never from the caller.
    pub async fn card(
        &self,
        thread_id: &str,
        card_id: &str,
        part: &str,
    ) -> Result<Option<Vec<u8>>> {
        if !crate::state::artifacts::is_card_id(card_id) {
            return Ok(None);
        }
        let launcher = self.of(thread_id).await;
        if !launcher.state.carries_card(thread_id, card_id).await? {
            return Ok(None);
        }
        let store = crate::state::artifacts::ArtifactStore::new(launcher.paths.artifacts_dir());
        // A file read blocks; keep it off the loop's thread.
        let (card_id, part) = (card_id.to_owned(), part.to_owned());
        let bytes = tokio::task::spawn_blocking(move || store.read_part(&card_id, &part).ok());
        Ok(bytes.await?)
    }

    pub async fn pin_source(&self, thread_id: &str, id: &str, pinned: bool) -> Result<()> {
        let launcher = self.of(thread_id).await;
        launcher.state.pin_source(thread_id, id, pinned).await
    }

    /// Forgets a source and deletes its file.
    pub async fn remove_source(&self, thread_id: &str, id: &str) -> Result<()> {
        let launcher = self.of(thread_id).await;
        let workspace = start::workspace(&launcher.state, &launcher.paths, thread_id).await?;
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
        let workspace = start::workspace(state, &launcher.paths, thread_id).await?;
        crate::tools::read_plan(state, &workspace, thread_id).await
    }

    /// Rewrites the thread's plan as the user typed it; empty text removes it.
    pub async fn save_plan(&self, thread_id: &str, text: &str) -> Result<()> {
        let launcher = self.of(thread_id).await;
        let state = &launcher.state;
        let workspace = start::workspace(state, &launcher.paths, thread_id).await?;
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
            .ok_or_else(|| crate::client::Shown::new("no such thread"))?;
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

    /// The thread's change ledger, newest first, capped.
    pub async fn file_changes(
        &self,
        thread_id: &str,
    ) -> Result<Vec<dusk_protocol::project::FileChange>> {
        let launcher = self.of(thread_id).await;
        launcher.state.thread_changes(thread_id).await
    }

    /// How the thread's work stands: activity, changes and the newest log lines.
    pub async fn overview(&self, thread_id: &str) -> Result<dusk_protocol::thread::Overview> {
        let launcher = self.of(thread_id).await;
        launcher.state.overview(thread_id).await
    }

    /// A page of the thread's event log older than `before`, newest first.
    pub async fn events(
        &self,
        thread_id: &str,
        before: Option<i64>,
        limit: u32,
    ) -> Result<Vec<dusk_protocol::thread::LogEntry>> {
        let launcher = self.of(thread_id).await;
        launcher.state.events(thread_id, before, limit).await
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

    /// The approvals of stored chats, for the tests that ask one.
    #[cfg(test)]
    pub(crate) fn approvals(&self) -> std::sync::Arc<crate::tools::Approvals> {
        std::sync::Arc::clone(&self.launcher.approvals)
    }

    /// False when the request is no longer waiting, such as after its turn was stopped.
    ///
    /// Whether only the owner may answer request `id`: a change of the settings a tool
    /// proposed.
    pub fn approval_owner_only(&self, id: u64) -> bool {
        [&self.launcher, &self.temporary]
            .iter()
            .any(|launcher| launcher.approvals.owner_only(id))
    }

    /// Only the chat it was asked in, or the chat above the agent thread that asked,
    /// may answer: a request asked elsewhere counts as unknown.
    pub async fn answer_approval(&self, thread_id: &str, id: u64, decision: Decision) -> bool {
        for launcher in [&self.launcher, &self.temporary] {
            let Some(asked) = launcher.approvals.thread_of(id) else {
                continue;
            };
            let state = &launcher.state;
            let (asked_chat, named_chat) = match (
                state.root_thread(&asked).await,
                state.root_thread(thread_id).await,
            ) {
                (Ok(asked_chat), Ok(named_chat)) => (asked_chat, named_chat),
                (Err(error), _) | (_, Err(error)) => {
                    tracing::warn!("approval {id} not answered: {error:#}");
                    return false;
                }
            };
            return asked_chat == named_chat && launcher.approvals.answer(id, decision);
        }
        false
    }

    /// The requests still waiting in `thread_id` and in every agent below it, as the events
    /// that opened them in that thread: an agent's wrapped once for each agent between, as it
    /// came live, so a client that comes after they were asked can still answer them.
    pub async fn waiting_approvals(&self, thread_id: &str) -> Vec<ThreadEvent> {
        let mut events = Vec::new();
        for launcher in [&self.launcher, &self.temporary] {
            let state = &launcher.state;
            if state.root_thread(thread_id).await.is_err() {
                continue;
            }
            for (asked, request) in launcher.approvals.waiting() {
                let id = request.id;
                let mut event = ThreadEvent::Approval(request);
                let mut thread = asked;
                let mut below = false;
                for _ in 0..MAX_DEPTH {
                    if thread == thread_id {
                        below = true;
                        break;
                    }
                    let parent = match state.parent_thread(&thread).await {
                        Ok(parent) => parent,
                        Err(error) => {
                            tracing::warn!("approval {id} not told again: {error:#}");
                            None
                        }
                    };
                    let Some(parent) = parent else { break };
                    event = ThreadEvent::Subagent {
                        thread_id: thread,
                        event: Box::new(event),
                    };
                    thread = parent;
                }
                // Asked by a thread beside this one, or above it: not this thread's.
                if below {
                    events.push(event);
                }
            }
        }
        events
    }
}
