//! The core's requests as typed calls, so a view says what it wants and not which
//! response variant carries it. Every error is the core's own text.

use crate::link::request;
use dusk_protocol::{
    client::{Response, ThreadRequest},
    config::ApprovalPolicy,
    thread::{Message, Task, Thread},
    tools::Decision,
};

use super::*;

pub async fn threads() -> Res<Vec<Thread>> {
    ask!(thread(ThreadRequest::List), Response::Threads(t) => t)
}

pub async fn new_thread(cwd: String) -> Res<String> {
    let cwd = cwd.into();
    ask!(thread(ThreadRequest::New { cwd }), Response::Opened { thread_id } => thread_id)
}

pub async fn new_temporary_thread(cwd: String) -> Res<String> {
    let cwd = cwd.into();
    ask!(thread(ThreadRequest::NewTemporary { cwd }), Response::Opened { thread_id } => thread_id)
}

pub async fn resume(thread_id: &str, cwd: String) -> Res<String> {
    let (thread_id, cwd) = (thread_id.to_owned(), cwd.into());
    ask!(thread(ThreadRequest::Resume { thread_id, cwd }), Response::Opened { thread_id } => thread_id)
}

pub async fn fork(summary: String, cwd: String) -> Res<String> {
    let cwd = cwd.into();
    ask!(thread(ThreadRequest::Fork { summary, cwd }), Response::Opened { thread_id } => thread_id)
}

/// A new chat holding `thread_id`'s history up to and including `message_id`.
pub async fn fork_at(thread_id: &str, message_id: &str, cwd: String) -> Res<String> {
    let (thread_id, message_id, cwd) = (thread_id.to_owned(), message_id.to_owned(), cwd.into());
    ask!(
        thread(ThreadRequest::ForkAt { thread_id, message_id, cwd }),
        Response::Opened { thread_id } => thread_id
    )
}

/// Starts the session the agent proposed as `proposal_id`; answers with the new chat.
pub async fn start_proposal(thread_id: &str, proposal_id: &str) -> Res<String> {
    let (thread_id, proposal_id) = (thread_id.to_owned(), proposal_id.to_owned());
    ask!(
        thread(ThreadRequest::StartProposal {
            thread_id,
            proposal_id
        }),
        Response::Opened { thread_id } => thread_id
    )
}

pub async fn dismiss_proposal(thread_id: &str, proposal_id: &str) -> Res<()> {
    let (thread_id, proposal_id) = (thread_id.to_owned(), proposal_id.to_owned());
    done(thread(ThreadRequest::DismissProposal {
        thread_id,
        proposal_id,
    }))
    .await
}

pub async fn messages(thread_id: &str) -> Res<Vec<Message>> {
    let thread_id = thread_id.to_owned();
    ask!(thread(ThreadRequest::Messages { thread_id }), Response::Messages(m) => m)
}

pub async fn tasks(thread_id: &str) -> Res<Vec<Task>> {
    let thread_id = thread_id.to_owned();
    ask!(thread(ThreadRequest::Tasks { thread_id }), Response::Tasks(t) => t)
}

pub async fn send(thread_id: &str, text: String, now: bool) -> Res<()> {
    let thread_id = thread_id.to_owned();
    done(thread(ThreadRequest::Send {
        thread_id,
        text,
        now,
    }))
    .await
}

pub async fn cancel(thread_id: &str) -> Res<()> {
    done(thread(ThreadRequest::Cancel {
        thread_id: thread_id.to_owned(),
    }))
    .await
}

pub async fn close(thread_id: &str) -> Res<()> {
    done(thread(ThreadRequest::Close {
        thread_id: thread_id.to_owned(),
    }))
    .await
}

pub async fn stop_agent(thread_id: &str) -> Res<()> {
    done(thread(ThreadRequest::StopAgent {
        thread_id: thread_id.to_owned(),
    }))
    .await
}

pub async fn rename(thread_id: &str, title: String) -> Res<()> {
    done(thread(ThreadRequest::Rename {
        thread_id: thread_id.to_owned(),
        title,
    }))
    .await
}

pub async fn delete_thread(thread_id: &str) -> Res<()> {
    done(thread(ThreadRequest::Delete {
        thread_id: thread_id.to_owned(),
    }))
    .await
}

pub async fn approve(thread_id: String, id: u64, decision: Decision) -> Res<bool> {
    ask!(thread(ThreadRequest::Approve { thread_id, id, decision }), Response::Waiting(w) => w)
}

pub async fn set_approvals(thread_id: &str, policy: Option<ApprovalPolicy>) -> Res<()> {
    done(thread(ThreadRequest::SetApprovals {
        thread_id: thread_id.to_owned(),
        policy,
    }))
    .await
}

pub async fn get_approvals(thread_id: &str) -> Res<Option<ApprovalPolicy>> {
    let thread_id = thread_id.to_owned();
    ask!(thread(ThreadRequest::GetApprovals { thread_id }), Response::Approvals(a) => a)
}

/// What the thread's agents changed in skills, oldest first.
pub async fn skill_changes(thread_id: &str) -> Res<Vec<dusk_protocol::catalog::SkillChange>> {
    let thread_id = thread_id.to_owned();
    ask!(thread(ThreadRequest::SkillChanges { thread_id }), Response::SkillChanges(c) => c)
}

/// The thread's pins, oldest message first.
pub async fn pins(thread_id: &str) -> Res<Vec<dusk_protocol::thread::Pin>> {
    let thread_id = thread_id.to_owned();
    ask!(thread(ThreadRequest::Pins { thread_id }), Response::Pins(p) => p)
}

/// Pins or unpins one message; answers with the thread's pins.
pub async fn set_pinned(
    thread_id: &str,
    message_id: &str,
    pinned: bool,
) -> Res<Vec<dusk_protocol::thread::Pin>> {
    let (thread_id, message_id) = (thread_id.to_owned(), message_id.to_owned());
    ask!(
        thread(ThreadRequest::Pin { thread_id, message_id, pinned }),
        Response::Pins(p) => p
    )
}

/// The chat is shown to the user: it is read.
pub async fn seen(thread_id: &str) -> Res<()> {
    done(thread(ThreadRequest::Seen {
        thread_id: thread_id.to_owned(),
    }))
    .await
}
