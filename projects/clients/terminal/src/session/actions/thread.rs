//! Actions on the thread on screen: its turns, agents and access mode.

use std::env;

use dusk_core::client::Client;
use dusk_protocol::{client::Command, config::ApprovalPolicy, tools::Decision};

use crate::app::{AgentWindow, App, Panel, Transcript};
use crate::session::{
    answers::answer,
    thread::{OpenThread, close, ensure_open},
};

pub async fn approve(client: &Client, app: &mut App, thread_id: &str, id: u64, decision: Decision) {
    if !matches!(
        client.threads().approve(thread_id, id, decision).await,
        Ok(true)
    ) {
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
        "auto" | "arbiter" => Some(ApprovalPolicy::Arbiter),
        "full" | "full access" | "full_access" => Some(ApprovalPolicy::FullAccess),
        _ => {
            app.flash = Some("Use ask, sandbox, auto, full or default".into());
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
            let unknown = card.is_some_and(|card| card.limit == 0);
            let window = AgentWindow::new(thread_id.clone(), parent, transcript);
            app.panel = Some(Panel::Agent(window));
            if unknown {
                read_agent_context(client, app, thread_id).await;
            }
        }
        Err(error) => app.error(format!("cannot open the agent: {error:#}")),
    }
}

/// Asks the core how full the context of agent `thread_id` is: one that worked before the
/// chat was open here sent its numbers to no one. The line is an extra; without it the
/// window stands.
async fn read_agent_context(client: &Client, app: &mut App, thread_id: String) {
    use dusk_protocol::client::Answer;
    let command = Command::Context {
        thread_id: Some(thread_id.clone()),
    };
    match client.command(command).await {
        Ok(Answer::Context(report)) => {
            if let Some(card) = app
                .subagents
                .iter_mut()
                .find(|card| card.thread_id == thread_id)
            {
                card.context_report(&report);
            }
        }
        Ok(_) => {}
        Err(error) => tracing::warn!("no context for agent {thread_id}: {error:#}"),
    }
}

/// `/pins` lists the chat's pins, `/pin` pins the agent's last answer, `/unpin N` takes
/// pin N away.
pub async fn pin(client: &Client, app: &mut App, ask: crate::app::PinAsk) {
    use crate::app::PinAsk;
    use dusk_protocol::thread::MessageRole;
    let Some(thread_id) = app.thread_id.clone() else {
        app.flash = Some("Send a message first; pins belong to a chat".into());
        return;
    };
    let threads = client.threads();
    let result = match ask {
        PinAsk::List => threads.pins(&thread_id).await,
        PinAsk::Last => {
            let last = threads.messages(&thread_id).await.map(|messages| {
                messages
                    .into_iter()
                    .rev()
                    .find(|message| {
                        message.role == MessageRole::Assistant && !message.content.trim().is_empty()
                    })
                    .map(|message| message.message_id)
            });
            match last {
                Ok(Some(message_id)) => threads.pin(&thread_id, &message_id, true).await,
                Ok(None) => {
                    app.flash = Some("No answer to pin yet".into());
                    return;
                }
                Err(error) => Err(error),
            }
        }
        PinAsk::Unpin(number) => match app.pins.get(number - 1) {
            Some(pin) => threads.pin(&thread_id, &pin.message_id, false).await,
            None => {
                app.flash = Some(format!("No pin {number}; /pins lists them"));
                return;
            }
        },
    };
    match result {
        Ok(pins) => {
            app.pins = pins;
            if app.pins.is_empty() {
                app.flash = Some("No pinned messages".into());
            }
            for (n, pin) in app.pins.iter().enumerate() {
                let who = match pin.role {
                    MessageRole::User => "you",
                    _ => "agent",
                };
                let folded = if pin.compacted { " (compacted)" } else { "" };
                let text = format!("pin {}  {who}: {}{folded}", n + 1, pin.excerpt);
                app.transcript.notice(text, crate::app::Tone::Info);
            }
        }
        // Such as "Unpin one first".
        Err(error) => app.flash = Some(format!("{error:#}")),
    }
}
