//! Actions on the thread on screen: its turns, agents and access mode.

use std::env;

use dusk_core::client::Client;
use dusk_protocol::{client::Command, config::ApprovalPolicy, tools::Decision};

use crate::app::{AgentWindow, App, Panel, Transcript};
use crate::session::{
    answers::answer,
    thread::{OpenThread, close, ensure_open},
};

pub async fn approve(client: &Client, app: &mut App, id: u64, decision: Decision) {
    if !matches!(client.threads().approve(id, decision).await, Ok(true)) {
        app.flash = Some("that request is no longer waiting".into());
    }
}

pub async fn access(client: &Client, app: &mut App, name: Option<String>) {
    let Some(thread_id) = app.thread_id.clone() else {
        app.flash = Some("Send a message first; the access mode belongs to a chat".into());
        return;
    };
    let Some(name) = name else {
        match client.threads().approvals(&thread_id).await {
            Ok(Some(policy)) => app.flash = Some(format!("Access here: {}", policy.name())),
            Ok(None) => {
                let default = app.status.approvals.name();
                app.flash = Some(format!("Access here: {default} (the default)"));
            }
            Err(error) => app.error(format!("cannot read the access mode: {error:#}")),
        }
        return;
    };
    let policy = match name.as_str() {
        "default" => None,
        "ask" => Some(ApprovalPolicy::Ask),
        "sandbox" => Some(ApprovalPolicy::Sandbox),
        "arbiter" => Some(ApprovalPolicy::Arbiter),
        "full" | "full access" | "full_access" => Some(ApprovalPolicy::FullAccess),
        _ => {
            app.flash = Some("Use ask, sandbox, arbiter, full or default".into());
            return;
        }
    };
    match client.threads().set_approvals(&thread_id, policy).await {
        Ok(()) => {
            app.flash = Some(match policy {
                Some(policy) => format!("Access here: {}", policy.name()),
                None => "Access here: back to the default".to_owned(),
            })
        }
        Err(error) => app.error(format!("cannot set the access mode: {error:#}")),
    }
}

pub async fn resend(client: &Client, app: &mut App, open: &mut Option<OpenThread>, id: String) {
    if client.status.borrow().model.is_none() {
        app.flash = Some("Connect a provider and choose a model before resending".into());
        return;
    }
    if let Err(error) = ensure_open(client, app, open).await {
        app.error(format!("cannot open the thread: {error:#}"));
        return;
    }
    if let Some(thread_id) = &app.thread_id {
        match client.threads().resend_local_unsent(thread_id, &id).await {
            Ok(()) => {
                app.mark_local_sent(&id);
                app.flash = Some("Local message queued".into());
            }
            Err(error) => app.error(format!("cannot resend the local message: {error:#}")),
        }
    }
}

pub async fn cancel(client: &Client, app: &mut App, open: &Option<OpenThread>) {
    if let Some(thread) = &open
        && let Err(error) = client.threads().cancel(&thread.thread_id).await
    {
        app.error(format!("{error:#}"));
    }
}

pub async fn undo(
    client: &Client,
    app: &mut App,
    open: &mut Option<OpenThread>,
    sequence: Option<u64>,
) {
    let thread_id = app.thread_id.clone();
    let command = Command::Undo {
        thread_id,
        sequence,
    };
    answer(client, app, open, command).await;
}

pub async fn resume(client: &Client, app: &mut App, open: &mut Option<OpenThread>) {
    let cwd = env::current_dir().ok();
    answer(client, app, open, Command::Resume { cwd }).await;
}

pub async fn context(client: &Client, app: &mut App, open: &mut Option<OpenThread>) {
    let thread_id = app.thread_id.clone();
    answer(client, app, open, Command::Context { thread_id }).await;
}

pub async fn stop_agent(client: &Client, app: &mut App, thread_id: String) {
    match client.threads().stop_agent(&thread_id).await {
        Ok(()) => app.flash = Some("stopping the agent".into()),
        Err(error) => app.error(format!("cannot stop the agent: {error:#}")),
    }
}

pub async fn open_thread(
    client: &Client,
    app: &mut App,
    open: &mut Option<OpenThread>,
    thread_id: String,
) {
    close(client, open.take()).await;
    match OpenThread::resume(client, app, &thread_id).await {
        Ok(thread) => *open = Some(thread),
        Err(error) => {
            app.open(None, None, &[]);
            app.error(format!("cannot open the thread: {error:#}"));
        }
    }
}

pub async fn open_agent(client: &Client, app: &mut App, thread_id: String) {
    match client.threads().messages(&thread_id).await {
        Ok(history) => {
            let transcript = Transcript::from_history(&history);
            let card = app
                .subagents
                .iter()
                .find(|card| card.thread_id == thread_id);
            let parent = card.and_then(|card| card.parent.clone());
            let window = AgentWindow::new(thread_id, parent, transcript);
            app.panel = Some(Panel::Agent(window));
        }
        Err(error) => app.error(format!("cannot open the agent: {error:#}")),
    }
}
