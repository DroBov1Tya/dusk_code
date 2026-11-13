//! Opening, feeding and closing threads.

use super::{Client, unexpected};
use anyhow::Result;
use dusk_protocol::{
    client::{Request, Response, ThreadRequest},
    config::ApprovalPolicy,
    thread::{LocalUnsent, Message, Task, Thread},
    tools::Decision,
};
use std::path::PathBuf;

/// The calls on threads; events of an open thread arrive on `Client::events`.
pub struct Threads<'a>(pub(super) &'a Client);

impl Threads<'_> {
    /// A new thread in `cwd`; it is created now and opened for messages.
    pub async fn start(&self, cwd: PathBuf) -> Result<String> {
        self.opened(ThreadRequest::New { cwd }).await
    }

    pub async fn resume(&self, thread_id: &str, cwd: PathBuf) -> Result<String> {
        let thread_id = thread_id.to_owned();
        self.opened(ThreadRequest::Resume { thread_id, cwd }).await
    }

    pub async fn fork(&self, summary: &str, cwd: PathBuf) -> Result<String> {
        let summary = summary.to_owned();
        self.opened(ThreadRequest::Fork { summary, cwd }).await
    }

    pub async fn list(&self) -> Result<Vec<Thread>> {
        match self.call(ThreadRequest::List).await? {
            Response::Threads(threads) => Ok(threads),
            other => Err(unexpected(other)),
        }
    }

    pub async fn search(&self, text: &str) -> Result<Vec<dusk_protocol::thread::Found>> {
        let text = text.to_owned();
        match self.call(ThreadRequest::Search { text }).await? {
            Response::Found(found) => Ok(found),
            other => Err(unexpected(other)),
        }
    }

    pub async fn messages(&self, thread_id: &str) -> Result<Vec<Message>> {
        let thread_id = thread_id.to_owned();
        match self.call(ThreadRequest::Messages { thread_id }).await? {
            Response::Messages(messages) => Ok(messages),
            other => Err(unexpected(other)),
        }
    }

    pub async fn tasks(&self, thread_id: &str) -> Result<Vec<Task>> {
        let thread_id = thread_id.to_owned();
        match self.call(ThreadRequest::Tasks { thread_id }).await? {
            Response::Tasks(tasks) => Ok(tasks),
            other => Err(unexpected(other)),
        }
    }

    pub async fn local_unsent(&self, thread_id: &str) -> Result<Vec<LocalUnsent>> {
        let thread_id = thread_id.to_owned();
        match self.call(ThreadRequest::LocalUnsent { thread_id }).await? {
            Response::LocalUnsent(messages) => Ok(messages),
            other => Err(unexpected(other)),
        }
    }

    pub async fn resend_local_unsent(&self, thread_id: &str, id: &str) -> Result<()> {
        let (thread_id, id) = (thread_id.to_owned(), id.to_owned());
        self.done(ThreadRequest::ResendLocalUnsent { thread_id, id })
            .await
    }

    /// Queues `text`, or puts it before what is queued when `now`.
    pub async fn send(&self, thread_id: &str, text: String, now: bool) -> Result<()> {
        let thread_id = thread_id.to_owned();
        self.done(ThreadRequest::Send {
            thread_id,
            text,
            now,
        })
        .await
    }

    /// Sends the agent's running command of `call_id` to the background.
    pub async fn send_to_background(&self, thread_id: &str, call_id: &str) -> Result<()> {
        self.done(ThreadRequest::Background {
            thread_id: thread_id.to_owned(),
            call_id: call_id.to_owned(),
        })
        .await
    }

    /// Runs a command the user confirmed, in the chat's folder; returns its output.
    pub async fn run_command(&self, thread_id: &str, command: &str) -> Result<String> {
        let request = ThreadRequest::RunCommand {
            thread_id: thread_id.to_owned(),
            command: command.to_owned(),
        };
        match self.call(request).await? {
            Response::Text(output) => Ok(output),
            other => Err(anyhow::anyhow!("unexpected answer: {other:?}")),
        }
    }

    pub async fn cancel(&self, thread_id: &str) -> Result<()> {
        let thread_id = thread_id.to_owned();
        self.done(ThreadRequest::Cancel { thread_id }).await
    }

    /// Stops the thread's turn and waits for its task to end.
    pub async fn close(&self, thread_id: &str) -> Result<()> {
        let thread_id = thread_id.to_owned();
        self.done(ThreadRequest::Close { thread_id }).await
    }

    pub async fn stop_agent(&self, thread_id: &str) -> Result<()> {
        let thread_id = thread_id.to_owned();
        self.done(ThreadRequest::StopAgent { thread_id }).await
    }

    pub async fn rename(&self, thread_id: &str, title: &str) -> Result<()> {
        let (thread_id, title) = (thread_id.to_owned(), title.to_owned());
        self.done(ThreadRequest::Rename { thread_id, title }).await
    }

    /// Erases the chat for good: its records, its agents' threads and its folder.
    pub async fn delete(&self, thread_id: &str) -> Result<()> {
        let thread_id = thread_id.to_owned();
        self.done(ThreadRequest::Delete { thread_id }).await
    }

    /// Answers a tool's request; `false` when it no longer waits.
    pub async fn approve(&self, id: u64, decision: Decision) -> Result<bool> {
        match self.call(ThreadRequest::Approve { id, decision }).await? {
            Response::Waiting(waiting) => Ok(waiting),
            other => Err(unexpected(other)),
        }
    }

    /// The thread's own approval mode; `None` goes back to the default in settings.
    pub async fn set_approvals(
        &self,
        thread_id: &str,
        policy: Option<ApprovalPolicy>,
    ) -> Result<()> {
        let thread_id = thread_id.to_owned();
        self.done(ThreadRequest::SetApprovals { thread_id, policy })
            .await
    }

    /// The thread's own approval mode; `None` when it uses the default in settings.
    pub async fn approvals(&self, thread_id: &str) -> Result<Option<ApprovalPolicy>> {
        let thread_id = thread_id.to_owned();
        match self.call(ThreadRequest::GetApprovals { thread_id }).await? {
            Response::Approvals(policy) => Ok(policy),
            other => Err(unexpected(other)),
        }
    }

    async fn opened(&self, request: ThreadRequest) -> Result<String> {
        match self.call(request).await? {
            Response::Opened { thread_id } => Ok(thread_id),
            other => Err(unexpected(other)),
        }
    }

    async fn call(&self, request: ThreadRequest) -> Result<Response> {
        self.0.call(Request::Thread(request)).await
    }

    async fn done(&self, request: ThreadRequest) -> Result<()> {
        self.0.done(Request::Thread(request)).await
    }
}
