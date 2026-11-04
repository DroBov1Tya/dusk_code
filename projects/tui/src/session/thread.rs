//! The thread the screen shows: opened when its first message is sent, picked up
//! again by /resume, and closed with the client.

use std::env;

use anyhow::Result;
use dusk_core::{
    runtime::{Runtime, ThreadEvent, ThreadHandle},
    state::TurnStatus,
};

use super::run::{CLOSE_TIMEOUT, THREAD_SOURCE};
use crate::app::App;

pub struct OpenThread {
    pub handle: ThreadHandle,
    /// The thread's task ended; the next message opens it again.
    pub ended: bool,
    pub active_turn: Option<String>,
    pub alerted: bool,
}

impl OpenThread {
    /// Opens the thread again with its history, tasks and unsent messages shown.
    pub async fn resume(runtime: &Runtime, app: &mut App, thread_id: &str) -> Result<Self> {
        let history = runtime.thread_messages(thread_id).await?;
        let threads = runtime.list_threads().await?;
        let title = threads
            .into_iter()
            .find(|thread| thread.id == thread_id)
            .and_then(|thread| thread.title);
        let handle = runtime
            .resume_thread(thread_id, &env::current_dir()?)
            .await?;
        app.open(Some(thread_id.to_owned()), title, &history);
        app.tasks = runtime.thread_tasks(thread_id).await?;
        for message in runtime.local_unsent(thread_id).await? {
            app.show_local_unsent(message);
        }
        Ok(Self::new(handle))
    }

    pub fn new(handle: ThreadHandle) -> Self {
        Self {
            handle,
            ended: false,
            active_turn: None,
            alerted: false,
        }
    }

    /// Only live root events count: history, child agents and intermediate errors
    /// never ring. Consuming the terminal event first also deduplicates a fatal error
    /// and a finish for the same turn, including while notifications are muted.
    pub fn apply(&mut self, app: &mut App, event: ThreadEvent) {
        let notify = match &event {
            ThreadEvent::TurnStarted(turn) => {
                self.active_turn = Some(turn.turn_id.clone());
                self.alerted = false;
                false
            }
            ThreadEvent::TurnFinished(turn)
                if self.active_turn.as_deref() == Some(turn.turn_id.as_str()) =>
            {
                self.active_turn = None;
                let notify = !self.alerted
                    && matches!(turn.status, TurnStatus::Completed | TurnStatus::Failed);
                self.alerted = true;
                notify
            }
            ThreadEvent::Failed(_) => {
                self.active_turn = None;
                let notify = !self.alerted;
                self.alerted = true;
                notify
            }
            ThreadEvent::Idle => {
                // The previous request has settled. A later standalone runner failure
                // is a new notification, even if no new model turn has started.
                self.active_turn = None;
                self.alerted = false;
                false
            }
            _ => false,
        };
        app.apply(event);
        if notify && app.config.current().sound_notifications() {
            crate::host::terminal::bell();
        }
    }
}

/// The next event of the open thread; waits forever when none is open.
pub async fn next_event(open: &mut Option<OpenThread>) -> Option<ThreadEvent> {
    match open {
        Some(thread) if !thread.ended => {
            let event = thread.handle.output.next_event().await;
            thread.ended = event.is_none();
            event
        }
        _ => std::future::pending().await,
    }
}

pub async fn send(
    runtime: &Runtime,
    app: &mut App,
    open: &mut Option<OpenThread>,
    text: String,
    now: bool,
) {
    if let Err(error) = ensure_open(runtime, app, open).await {
        app.unsend(text);
        app.error(format!("cannot open a thread: {error:#}"));
        return;
    }
    let Some(thread) = open else {
        return;
    };
    // Long pastes and named pictures leave the message for the thread's sources; the
    // folder exists by now.
    let text = match runtime.keep_pastes(&thread.handle.thread_id, &text).await {
        Ok(kept) => kept,
        Err(error) => {
            app.unsend(text);
            app.error(format!("cannot keep the paste: {error:#}"));
            return;
        }
    };
    let input = &thread.handle.input;
    let sent = if now {
        input.send_now(text.clone()).await
    } else {
        input.send(text.clone()).await
    };
    if let Err(error) = sent {
        app.unsend(text);
        app.error(format!("{error:#}"));
    }
}

/// Creates the thread on its first message, or reopens one whose task has ended.
pub async fn ensure_open(
    runtime: &Runtime,
    app: &mut App,
    open: &mut Option<OpenThread>,
) -> Result<()> {
    let cwd = env::current_dir()?;
    match open.take() {
        Some(thread) if !thread.ended => *open = Some(thread),
        Some(thread) => {
            let thread_id = thread.handle.thread_id.clone();
            close(Some(thread)).await;
            let handle = runtime.resume_thread(&thread_id, &cwd).await?;
            *open = Some(OpenThread::new(handle));
        }
        None => {
            let handle = runtime.new_thread(THREAD_SOURCE, &cwd).await?;
            app.thread_id = Some(handle.thread_id.clone());
            *open = Some(OpenThread::new(handle));
        }
    }
    Ok(())
}

/// Stops the thread's turn and waits for its task to end, so it can be opened again.
pub async fn close(open: Option<OpenThread>) {
    let Some(OpenThread { handle, ended, .. }) = open else {
        return;
    };
    let ThreadHandle {
        input, mut output, ..
    } = handle;
    if ended {
        return;
    }
    // Cancelling a thread that already ended has nothing to stop.
    let _ = input.cancel().await;
    drop(input);
    let drained = async { while output.next_event().await.is_some() {} };
    let _ = tokio::time::timeout(CLOSE_TIMEOUT, drained).await;
}
