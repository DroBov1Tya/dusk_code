//! The thread the screen shows: opened when its first message is sent, picked up
//! again by /resume, and closed with the client.

use std::env;

use anyhow::Result;
use dusk_core::client::Client;
use dusk_protocol::{thread::ThreadEvent, work::TurnStatus};

use crate::app::App;

pub struct OpenThread {
    pub thread_id: String,
    /// The thread's task ended; the next message opens it again.
    pub ended: bool,
    pub active_turn: Option<String>,
    pub alerted: bool,
}

impl OpenThread {
    /// Opens the thread again with its history, tasks and unsent messages shown.
    pub async fn resume(client: &Client, app: &mut App, thread_id: &str) -> Result<Self> {
        let threads = client.threads();
        let history = threads.messages(thread_id).await?;
        let title = threads
            .list()
            .await?
            .into_iter()
            .find(|thread| thread.id == thread_id)
            .and_then(|thread| thread.title);
        let thread_id = threads.resume(thread_id, env::current_dir()?).await?;
        app.open(Some(thread_id.clone()), title, &history);
        app.tasks = threads.tasks(&thread_id).await?;
        for message in threads.local_unsent(&thread_id).await? {
            app.show_local_unsent(message);
        }
        Ok(Self::new(thread_id))
    }

    pub fn new(thread_id: String) -> Self {
        Self {
            thread_id,
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
        if notify && app.status.sound_notifications {
            crate::host::terminal::bell();
        }
    }
}

pub async fn send(
    client: &Client,
    app: &mut App,
    open: &mut Option<OpenThread>,
    text: String,
    now: bool,
) {
    if let Err(error) = ensure_open(client, app, open).await {
        app.unsend(text);
        app.error(format!("cannot open a thread: {error:#}"));
        return;
    }
    let Some(thread) = open else {
        return;
    };
    // Long pastes and named pictures leave the message for the thread's sources there.
    let sent = client
        .threads()
        .send(&thread.thread_id, text.clone(), now)
        .await;
    if let Err(error) = sent {
        app.unsend(text);
        app.error(format!("{error:#}"));
    }
}

/// Creates the thread on its first message, or reopens one whose task has ended.
pub async fn ensure_open(
    client: &Client,
    app: &mut App,
    open: &mut Option<OpenThread>,
) -> Result<()> {
    let cwd = env::current_dir()?;
    match open.take() {
        Some(thread) if !thread.ended => *open = Some(thread),
        Some(thread) => {
            let thread_id = client.threads().resume(&thread.thread_id, cwd).await?;
            *open = Some(OpenThread::new(thread_id));
        }
        None => {
            let thread_id = client.threads().start(cwd).await?;
            app.thread_id = Some(thread_id.clone());
            *open = Some(OpenThread::new(thread_id));
        }
    }
    Ok(())
}

/// Stops the thread's turn and waits for its task to end, so it can be opened again.
pub async fn close(client: &Client, open: Option<OpenThread>) {
    let Some(thread) = open else {
        return;
    };
    if !thread.ended {
        let _ = client.threads().close(&thread.thread_id).await;
    }
}
