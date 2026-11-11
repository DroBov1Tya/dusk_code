//! Requests about one thread: opening, sending, closing.

use super::{CLOSE_TIMEOUT, OpenThread, Server};
use super::{ClientId, Subscribers};
use crate::runtime::ThreadHandle;
use anyhow::{Context, Result, ensure};
use dusk_protocol::client::{Event, Response, ThreadRequest};
use std::{collections::HashSet, sync::Arc};
use tracing::warn;

impl Server {
    pub(super) async fn thread(&mut self, request: ThreadRequest) -> Result<Response> {
        let runtime = Arc::clone(&self.runtime);
        Ok(match request {
            ThreadRequest::New { cwd } => {
                let handle = runtime.new_thread(self.source(), &cwd).await?;
                self.open(handle)
            }
            ThreadRequest::Resume { thread_id, cwd } => {
                // A thread another client keeps open runs on; this one just joins it.
                if let Some(open) = self.threads.get_mut(&thread_id)
                    && !open.forward.is_finished()
                {
                    open.clients.insert(self.caller);
                    open.orphaned = None;
                    return Ok(Response::Opened { thread_id });
                }
                self.close(&thread_id).await;
                let handle = runtime.resume_thread(&thread_id, &cwd).await?;
                self.open(handle)
            }
            ThreadRequest::Fork { summary, cwd } => {
                let handle = runtime.fork_thread(self.source(), &summary, &cwd).await?;
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
                self.leave(&thread_id).await;
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
        let subscribers = Arc::clone(&self.subscribers);
        let id = thread_id.clone();
        let forward = tokio::spawn(async move {
            while let Some(event) = output.next_event().await {
                let event = Event::Thread {
                    thread_id: id.clone(),
                    event: Box::new(event),
                };
                publish(&subscribers, event);
            }
            publish(&subscribers, Event::Ended { thread_id: id });
        });
        let clients = HashSet::from([self.caller]);
        self.threads.insert(
            thread_id.clone(),
            OpenThread {
                input,
                forward,
                clients,
                orphaned: None,
            },
        );
        Response::Opened { thread_id }
    }

    pub(super) fn open_thread(&self, thread_id: &str) -> Result<&OpenThread> {
        self.threads
            .get(thread_id)
            .with_context(|| format!("thread {thread_id} is not open"))
    }

    /// Stops the thread's turn and waits for its task to end, so it can be opened again.
    pub(super) async fn close(&mut self, thread_id: &str) {
        let Some(OpenThread { input, forward, .. }) = self.threads.remove(thread_id) else {
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

impl Server {
    /// The caller stops using the thread; it closes when no other client keeps it open.
    async fn leave(&mut self, thread_id: &str) {
        let caller = self.caller;
        self.release(thread_id, caller).await;
    }

    /// A client went away: it leaves every thread, and the ones nobody keeps close.
    pub(super) async fn detach(&mut self, client: ClientId) {
        self.sources.remove(&client);
        self.subscribers.lock().retain(|(id, _)| *id != client);
        let ids: Vec<String> = self.threads.keys().cloned().collect();
        // A client that is gone, unlike one that closes a thread, may be a browser that
        // reloads: its threads run on for a while before they are stopped.
        for thread_id in ids {
            if let Some(open) = self.threads.get_mut(&thread_id)
                && open.clients.remove(&client)
                && open.clients.is_empty()
            {
                open.orphaned = Some(std::time::Instant::now());
            }
        }
    }

    /// A client that asked to close the thread: it ends now if nobody else keeps it.
    async fn release(&mut self, thread_id: &str, client: ClientId) {
        let Some(open) = self.threads.get_mut(thread_id) else {
            return;
        };
        open.clients.remove(&client);
        if open.clients.is_empty() {
            self.close(thread_id).await;
        }
    }

    /// Stops the threads nobody has kept open for longer than the grace.
    pub(super) async fn close_orphans(&mut self) {
        let stale: Vec<String> = self
            .threads
            .iter()
            .filter(|(_, open)| {
                open.orphaned
                    .is_some_and(|at| at.elapsed() > super::ORPHAN_GRACE)
            })
            .map(|(id, _)| id.clone())
            .collect();
        for thread_id in stale {
            self.close(&thread_id).await;
        }
    }
}

/// Gives the event to every attached client, dropping the queues of the gone ones.
fn publish(subscribers: &Subscribers, event: Event) {
    subscribers
        .lock()
        .retain(|(_, queue)| queue.send(event.clone()).is_ok());
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::mpsc;

    fn ended(thread_id: &str) -> Event {
        Event::Ended {
            thread_id: thread_id.to_owned(),
        }
    }

    #[test]
    fn every_attached_client_gets_each_event_in_order() {
        let subscribers = Subscribers::default();
        let (first, mut first_events) = mpsc::unbounded_channel();
        let (second, mut second_events) = mpsc::unbounded_channel();
        subscribers.lock().push((1, first));
        subscribers.lock().push((2, second));

        publish(&subscribers, ended("a"));
        publish(&subscribers, ended("b"));

        for events in [&mut first_events, &mut second_events] {
            assert_eq!(events.try_recv().unwrap(), ended("a"));
            assert_eq!(events.try_recv().unwrap(), ended("b"));
        }
    }

    #[test]
    fn a_gone_client_is_dropped_and_the_others_keep_receiving() {
        let subscribers = Subscribers::default();
        let (gone, gone_events) = mpsc::unbounded_channel();
        let (stays, mut stays_events) = mpsc::unbounded_channel();
        subscribers.lock().push((1, gone));
        subscribers.lock().push((2, stays));
        drop(gone_events);

        publish(&subscribers, ended("a"));

        assert_eq!(subscribers.lock().len(), 1);
        assert_eq!(stays_events.try_recv().unwrap(), ended("a"));
    }
}
