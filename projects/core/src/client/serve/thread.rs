//! Requests about one thread: opening, sending, closing.

use super::{CLOSE_TIMEOUT, OpenThread, Server};
use crate::runtime::ThreadHandle;
use anyhow::{Context, Result, ensure};
use dusk_protocol::client::{Event, Response, ThreadRequest};
use std::sync::Arc;
use tracing::warn;

impl Server {
    pub(super) async fn thread(&mut self, request: ThreadRequest) -> Result<Response> {
        let runtime = Arc::clone(&self.runtime);
        Ok(match request {
            ThreadRequest::New { cwd } => {
                let handle = runtime.new_thread(&self.source, &cwd).await?;
                self.open(handle)
            }
            ThreadRequest::Resume { thread_id, cwd } => {
                self.close(&thread_id).await;
                let handle = runtime.resume_thread(&thread_id, &cwd).await?;
                self.open(handle)
            }
            ThreadRequest::Fork { summary, cwd } => {
                let handle = runtime.fork_thread(&self.source, &summary, &cwd).await?;
                self.open(handle)
            }
            ThreadRequest::List => Response::Threads(runtime.list_threads().await?),
            ThreadRequest::Messages { thread_id } => {
                Response::Messages(runtime.thread_messages(&thread_id).await?)
            }
            ThreadRequest::Tasks { thread_id } => {
                Response::Tasks(runtime.thread_tasks(&thread_id).await?)
            }
            ThreadRequest::LocalUnsent { thread_id } => {
                Response::LocalUnsent(runtime.local_unsent(&thread_id).await?)
            }
            ThreadRequest::ResendLocalUnsent { thread_id, id } => {
                runtime.resend_local_unsent(&thread_id, &id).await?;
                Response::Done
            }
            ThreadRequest::Send {
                thread_id,
                text,
                now,
            } => {
                let open = self.open_thread(&thread_id)?;
                let text = runtime
                    .keep_pastes(&thread_id, &text)
                    .await
                    .context("cannot keep the paste")?;
                match now {
                    true => open.input.send_now(text).await?,
                    false => open.input.send(text).await?,
                }
                Response::Done
            }
            ThreadRequest::Background { thread_id, call_id } => {
                runtime.send_to_background(&thread_id, &call_id).await?;
                Response::Done
            }
            ThreadRequest::RunCommand { thread_id, command } => {
                Response::Text(runtime.run_for_user(&thread_id, &command).await?)
            }
            ThreadRequest::Cancel { thread_id } => {
                self.open_thread(&thread_id)?.input.cancel().await?;
                Response::Done
            }
            ThreadRequest::Close { thread_id } => {
                self.close(&thread_id).await;
                Response::Done
            }
            ThreadRequest::StopAgent { thread_id } => {
                runtime.stop_agent(&thread_id).await?;
                Response::Done
            }
            ThreadRequest::Rename { thread_id, title } => {
                let title = title.trim();
                ensure!(
                    title.chars().count() <= 200,
                    "a title is at most 200 characters"
                );
                runtime.state().set_title(&thread_id, title).await?;
                Response::Done
            }
            ThreadRequest::Delete { thread_id } => {
                // Its turn ends first, so nothing writes to what is being erased.
                self.close(&thread_id).await;
                let runtime = &self.runtime;
                for folder in runtime.state().delete_thread(&thread_id).await? {
                    // Only plain names under the workspace root are taken.
                    let path = runtime.paths().workspace_folder(&folder)?;
                    match tokio::fs::remove_dir_all(&path).await {
                        Ok(()) => {}
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                        Err(error) => warn!("cannot remove a deleted chat's folder: {error}"),
                    }
                }
                Response::Done
            }
            ThreadRequest::Approve { id, decision } => {
                Response::Waiting(runtime.answer_approval(id, decision))
            }
            ThreadRequest::SetApprovals { thread_id, policy } => {
                runtime
                    .state()
                    .set_thread_approvals(&thread_id, policy)
                    .await?;
                Response::Done
            }
            ThreadRequest::GetApprovals { thread_id } => {
                Response::Approvals(runtime.state().thread_approvals(&thread_id).await?)
            }
        })
    }

    /// Keeps the thread's input and forwards its events until its task ends.
    pub(super) fn open(&mut self, handle: ThreadHandle) -> Response {
        let ThreadHandle {
            thread_id,
            input,
            mut output,
        } = handle;
        let events = self.events.clone();
        let id = thread_id.clone();
        let forward = tokio::spawn(async move {
            while let Some(event) = output.next_event().await {
                let event = Event::Thread {
                    thread_id: id.clone(),
                    event: Box::new(event),
                };
                if events.send(event).is_err() {
                    return;
                }
            }
            let _ = events.send(Event::Ended { thread_id: id });
        });
        self.threads
            .insert(thread_id.clone(), OpenThread { input, forward });
        Response::Opened { thread_id }
    }

    pub(super) fn open_thread(&self, thread_id: &str) -> Result<&OpenThread> {
        self.threads
            .get(thread_id)
            .with_context(|| format!("thread {thread_id} is not open"))
    }

    /// Stops the thread's turn and waits for its task to end, so it can be opened again.
    pub(super) async fn close(&mut self, thread_id: &str) {
        let Some(OpenThread { input, forward }) = self.threads.remove(thread_id) else {
            return;
        };
        if forward.is_finished() {
            return;
        }
        // Cancelling a thread that already ended has nothing to stop.
        let _ = input.cancel().await;
        drop(input);
        let _ = tokio::time::timeout(CLOSE_TIMEOUT, forward).await;
    }
}
