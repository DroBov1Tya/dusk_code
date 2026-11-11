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

pub async fn resume(thread_id: &str, cwd: String) -> Res<String> {
    let (thread_id, cwd) = (thread_id.to_owned(), cwd.into());
    ask!(thread(ThreadRequest::Resume { thread_id, cwd }), Response::Opened { thread_id } => thread_id)
}

pub async fn fork(summary: String, cwd: String) -> Res<String> {
    let cwd = cwd.into();
    ask!(thread(ThreadRequest::Fork { summary, cwd }), Response::Opened { thread_id } => thread_id)
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

pub async fn approve(id: u64, decision: Decision) -> Res<bool> {
    ask!(thread(ThreadRequest::Approve { id, decision }), Response::Waiting(w) => w)
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
