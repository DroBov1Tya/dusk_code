//! The threads of the tabs not on screen. Each stays open in the core with its turn
//! running; the core sends every client the events of every open thread, and each event
//! goes to the tab of its thread. Closing a tab stops nothing: a thread still at work is
//! closed in the core once its work ends.

use std::collections::HashMap;

use dusk_core::client::Client;
use dusk_protocol::{client::Event as CoreEvent, thread::ThreadEvent};

use super::thread::{OpenThread, close, send};
use crate::app::{Action, App, TabMove};

/// The open threads of the tabs not on screen, by thread id; the shown one is `open`.
pub type Held = HashMap<String, OpenThread>;

/// Does what a tab action asks, and shows an already open thread in its tab; any other
/// action is handed back.
pub async fn take(
    client: &Client,
    app: &mut App,
    open: &mut Option<OpenThread>,
    held: &mut Held,
    action: Action,
) -> Action {
    match action {
        Action::Tab(TabMove::Close) => close_shown(client, app, open, held).await,
        Action::Tab(step) => switch(app, open, held, |app| match step {
            TabMove::New => app.new_tab(),
            TabMove::Next => app.step_tab(true),
            TabMove::Previous => app.step_tab(false),
            TabMove::Go(place) => app.go_tab(place),
            TabMove::Close => {}
        }),
        Action::OpenThread(thread_id) if app.thread_id.as_deref() != Some(&thread_id) => {
            match app.tab_of(&thread_id) {
                Some(index) => switch(app, open, held, |app| {
                    app.tabs[index].closed = false;
                    app.select_tab(index);
                }),
                None => return Action::OpenThread(thread_id),
            }
        }
        other => return other,
    }
    Action::None
}

/// Gives an event to the tab of its thread; those of threads in no tab are dropped.
pub fn route(app: &mut App, open: &mut Option<OpenThread>, held: &mut Held, event: CoreEvent) {
    let (thread_id, event) = match event {
        CoreEvent::Thread { thread_id, event } => (thread_id, Some(*event)),
        CoreEvent::Ended { thread_id } => (thread_id, None),
        _ => return,
    };
    if let Some(thread) = open.as_mut().filter(|thread| thread.thread_id == thread_id) {
        return show(app, thread, event);
    }
    let (Some(index), Some(thread)) = (app.tab_of(&thread_id), held.get_mut(&thread_id)) else {
        return;
    };
    let finished = matches!(
        event,
        None | Some(ThreadEvent::TurnFinished(_) | ThreadEvent::Failed(_))
    );
    let asks = matches!(
        event,
        Some(ThreadEvent::Approval(_) | ThreadEvent::Question(_))
    );
    app.in_tab(index, |app| show(app, thread, event));
    let tab = &mut app.tabs[index];
    tab.unseen |= finished;
    // Its agent cannot go on without the user: the closed tab comes back.
    if asks && tab.closed {
        tab.closed = false;
        let title = tab.title.as_deref().unwrap_or("a closed tab");
        let note = format!("{title} asks you something; its tab is back");
        app.flash = Some(note);
    }
}

/// Sends what waits in the background tabs once their agent is free, and closes the
/// threads of closed tabs whose work has ended.
pub async fn tend(client: &Client, app: &mut App, held: &mut Held) {
    for index in 0..app.tabs.len() {
        let tab = &app.tabs[index];
        if index == app.tab || tab.closed || tab.working() || tab.queued.is_empty() {
            continue;
        }
        let Some(thread) = tab.thread_id.as_ref().and_then(|id| held.remove(id)) else {
            continue;
        };
        let mut thread = Some(thread);
        app.trade_tab(index);
        if let Some(text) = app.release_queue() {
            send(client, app, &mut thread, text, false).await;
        }
        app.trade_tab(index);
        if let Some(thread) = thread {
            held.insert(thread.thread_id.clone(), thread);
        }
    }
    for thread_id in app.forget_closed() {
        close(client, held.remove(&thread_id)).await;
    }
}

/// Closes every tab's thread as the client ends.
pub async fn close_all(client: &Client, open: Option<OpenThread>, held: &mut Held) {
    close(client, open).await;
    for (_, thread) in held.drain() {
        close(client, Some(thread)).await;
    }
}

fn show(app: &mut App, thread: &mut OpenThread, event: Option<ThreadEvent>) {
    match event {
        Some(event) => thread.apply(app, event),
        None => {
            thread.ended = true;
            app.error("the thread stopped; the next message opens it again");
            app.stop_work();
        }
    }
}

/// Moves to another tab with `step`, keeping the open thread of the one left.
fn switch(
    app: &mut App,
    open: &mut Option<OpenThread>,
    held: &mut Held,
    step: impl FnOnce(&mut App),
) {
    if let Some(thread) = open.take() {
        held.insert(thread.thread_id.clone(), thread);
    }
    step(app);
    *open = app.thread_id.as_ref().and_then(|id| held.remove(id));
}

/// Closes the shown tab. A thread at work stays open, off the strip, until its work
/// ends; what waits to be sent goes to the core's own queue first.
async fn close_shown(
    client: &Client,
    app: &mut App,
    open: &mut Option<OpenThread>,
    held: &mut Held,
) {
    let keep = app.working();
    if keep && let Some(text) = app.release_queue() {
        send(client, app, open, text, false).await;
    }
    let leaving = open.take();
    if !app.close_tab(keep) {
        *open = leaving;
        return;
    }
    match leaving {
        Some(thread) if keep => {
            held.insert(thread.thread_id.clone(), thread);
        }
        thread => close(client, thread).await,
    }
    *open = app.thread_id.as_ref().and_then(|id| held.remove(id));
}
