//! Unread chats: what of a chat's events is for its user, and telling every client that
//! lists the chat when it turns unread or read. A client marks a chat seen while it shows
//! it, so the core needs no idea of who looks.

use super::Subscribers;
use super::publish::publish;
use crate::runtime::Runtime;
use dusk_protocol::client::Event;
use dusk_protocol::thread::{ThreadEvent, TurnStatus};
use tracing::warn;

/// Whether the chat's event is something for its user: the end of a turn the user did
/// not stop (an answer, a question the model asked, a failure), a limit that stopped one
/// (the output cap, a broken answer, a context too large), a thread that stopped, and a
/// request for a yes, an agent's below it too. A turn the user stopped, by a stop or by
/// a message sent to go first, ends `Interrupted` with no limit before it: the user did
/// that and saw it. Their own messages, progress inside a turn and what agents say to
/// each other are not for them either.
fn for_the_user(event: &ThreadEvent) -> bool {
    match event {
        ThreadEvent::TurnFinished(turn) => turn.status != TurnStatus::Interrupted,
        ThreadEvent::OutputLimitReached
        | ThreadEvent::AnswerCut { .. }
        | ThreadEvent::ContextTooLarge { .. }
        | ThreadEvent::Failed(_)
        | ThreadEvent::Approval(_) => true,
        ThreadEvent::Subagent { event, .. } => asks_the_user(event),
        _ => false,
    }
}

/// Marks the chat unread when its event is for the user. Called before the event goes
/// out, so a client that shows the chat and marks it seen on hearing it is not undone.
pub(super) async fn heard(
    runtime: &Runtime,
    subscribers: &Subscribers,
    thread_id: &str,
    event: &ThreadEvent,
) {
    if for_the_user(event) {
        set(runtime, subscribers, thread_id, true).await;
    }
}

/// An agent's request for a yes, at any depth: the user answers it in the chat.
fn asks_the_user(event: &ThreadEvent) -> bool {
    match event {
        ThreadEvent::Approval(_) => true,
        ThreadEvent::Subagent { event, .. } => asks_the_user(event),
        _ => false,
    }
}

/// Stores that the chat is unread or read and, when that changed, tells its clients.
/// A failure to store it is logged: the chat itself goes on.
pub(super) async fn set(
    runtime: &Runtime,
    subscribers: &Subscribers,
    thread_id: &str,
    unread: bool,
) {
    let state = runtime.state_of(thread_id).await;
    let changed = match unread {
        true => state.mark_unread(thread_id).await,
        false => state.mark_seen(thread_id).await,
    };
    match changed {
        Ok(false) => {}
        Ok(true) => publish(
            subscribers,
            Event::Thread {
                thread_id: thread_id.to_owned(),
                event: Box::new(ThreadEvent::Unread(unread)),
            },
        ),
        Err(error) => warn!("thread {thread_id}: cannot store whether it is read: {error:#}"),
    }
}
