//! The loop that joins the screen and the core: keys, thread events and the spinner's
//! clock go in, the app changes, the screen is drawn again. A new thread is created only
//! when its first message is sent, so an opened and closed client leaves nothing behind.

use std::{
    env,
    future::Future,
    path::PathBuf,
    pin::Pin,
    time::{Duration, Instant},
};

use anyhow::Result;
use crossterm::event::{Event, EventStream, MouseEventKind};
use dusk_core::{app::ArtifactStore, client::Client};
use dusk_protocol::client::Event as CoreEvent;
use futures::StreamExt;
use ratatui::DefaultTerminal;

use super::{
    actions::{Flow, apply, set_update_note},
    answers::{jobs_wanted, refresh_jobs, refresh_system, system_open},
    thread::{OpenThread, close, send},
};
use crate::{
    app::{Action, App, PRESS, Panel, SettingCategory, Tone},
    screensaver, ui,
};

pub const THREAD_SOURCE: &str = "tui";
pub const FRAME: Duration = Duration::from_millis(80);
/// How often the git line is read again; edits made outside show within it.
pub const GIT_PERIOD: Duration = Duration::from_secs(5);
/// How often the process's memory is written to the log, to see a slow growth.
pub const MEMORY_LOG_PERIOD: Duration = Duration::from_secs(10 * 60);
/// How often the system tab reads its numbers again.
pub const SYSTEM_PERIOD: Duration = Duration::from_secs(2);
/// How often the jobs are read again while one runs or their window is open.
pub const JOBS_PERIOD: Duration = Duration::from_secs(1);
pub const MAX_EVENTS_PER_FRAME: usize = 512;

pub struct InstallingUpdate {
    pub tag: String,
    pub result: Pin<Box<dyn Future<Output = Result<PathBuf>> + Send>>,
}

pub async fn run(
    client: &mut Client,
    mut terminal: DefaultTerminal,
    thread: Option<String>,
) -> Result<()> {
    let status = client.status.borrow_and_update().clone();
    let mut app = App::new(status.clone());
    app.transcript
        .set_artifact_store(ArtifactStore::new(status.artifacts_dir.clone()));
    let mut open = None;
    if let Some(thread_id) = thread {
        open = Some(OpenThread::resume(client, &mut app, &thread_id).await?);
    }
    // Shipped files follow the build; a local edit to one is replaced, so say which.
    let updated = &status.updated_builtin;
    if !updated.is_empty() {
        let mut names = updated
            .iter()
            .take(5)
            .cloned()
            .collect::<Vec<_>>()
            .join(", ");
        if updated.len() > 5 {
            names.push_str(&format!(" and {} more", updated.len() - 5));
        }
        app.transcript.notice(
            format!(
                "shipped files brought to this version: {names}. Your own agents and \
                 skills belong beside them under another name"
            ),
            Tone::Info,
        );
    }
    let mut keys = EventStream::new();
    let mut clock = tokio::time::interval(FRAME);
    // The clock sleeps while idle; missed frames must not play back in a burst.
    clock.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut git_clock = tokio::time::interval(GIT_PERIOD);
    let mut system_clock = tokio::time::interval(SYSTEM_PERIOD);
    let mut jobs_clock = tokio::time::interval(JOBS_PERIOD);
    let mut memory_clock = tokio::time::interval_at(
        tokio::time::Instant::now() + MEMORY_LOG_PERIOD,
        MEMORY_LOG_PERIOD,
    );
    let cwd = env::current_dir()?;
    let mut shown_title = String::new();
    let mut terminal_status = crate::host::terminal::TerminalStatus::new();
    let mut known_model = status.model.clone();
    let mut known_chatgpt = status.chatgpt.clone();
    let mut update: Option<InstallingUpdate> = None;
    let mut idle_since = Instant::now();
    loop {
        let status = client.status.borrow_and_update().clone();
        if update.is_some()
            && let Some(Panel::Update { progress, .. }) = &mut app.panel
        {
            *progress = status.install;
        }
        if let Some(name) = &status.model {
            // A saved model is not proof the provider answers, but one the core picked
            // after start was found over the network. No thread may exist yet to say so.
            if known_model.as_ref() != Some(name) {
                app.provider_available = true;
                known_model = Some(name.clone());
            }
            app.model = name.clone();
            app.limit = status.input_limit;
        }
        // A sign-in ends in the browser; say here how it went.
        if known_chatgpt != status.chatgpt {
            use dusk_protocol::client::ChatGpt;
            match &status.chatgpt {
                ChatGpt::SignedIn { account } => app.transcript.notice(
                    format!("Signed in with ChatGPT as {account}; /model lists its models"),
                    Tone::Success,
                ),
                ChatGpt::Failed(error) => app
                    .transcript
                    .notice(format!("Sign-in with ChatGPT failed: {error}"), Tone::Error),
                ChatGpt::SignedOut | ChatGpt::Waiting => {}
            }
            known_chatgpt = status.chatgpt.clone();
        }
        app.effort = status.effort;
        app.mode = status.mode.clone();
        if let Some(Panel::Settings(view)) = &mut app.panel {
            view.selected_model = status.model.clone();
            view.context_policy = status.context_policy;
            view.effective_limit = status.input_limit;
            view.effort = app.effort;
        }
        let mcp_starting = status.mcp_starting;
        app.mcp = status.mcp.clone();
        if matches!(app.panel, Some(Panel::Plugins { .. })) {
            app.plugins = status
                .plugins
                .iter()
                .cloned()
                .map(|plugin| crate::app::PluginRow {
                    name: plugin.name,
                    version: plugin.version,
                    description: plugin.description,
                    enabled: plugin.enabled,
                    parts: plugin.parts,
                    note: plugin.problem,
                    secrets: plugin.secrets,
                })
                .collect();
        }
        // Servers start in the background; the settings show them as they settle.
        if let Some(Panel::Settings(view)) = &mut app.panel
            && view.category == Some(SettingCategory::Mcp)
            && let Ok(servers) = client.catalog().mcp_servers().await
        {
            view.mcp = servers;
            if let Some(editor) = &mut view.editor {
                editor.update_mcp(&view.mcp);
            }
        }
        app.status = status;
        // /undo changed the history: it is read again, and the message goes back to the box.
        if let Some(text) = app.undone.take()
            && let Some(thread_id) = app.thread_id.clone()
        {
            match client.threads().messages(&thread_id).await {
                Ok(history) => {
                    let (title, goal) = (app.title.take(), app.goal.take());
                    let (tasks, tokens) = (std::mem::take(&mut app.tasks), app.tokens);
                    app.open(Some(thread_id), title, &history);
                    (app.goal, app.tasks, app.tokens) = (goal, tasks, tokens);
                    app.input.set(text);
                    app.flash = Some("went back; the message is in the box".into());
                }
                Err(error) => app.error(format!("cannot read the thread again: {error:#}")),
            }
        }
        // A fork summary opens its new thread; this one stays as it was.
        if let Some(summary) = app.forked.take() {
            close(client, open.take()).await;
            match client.threads().fork(&summary, cwd.clone()).await {
                Ok(thread_id) => {
                    app.open(Some(thread_id.clone()), None, &[]);
                    app.transcript.notice(
                        "forked: this thread goes on from a summary of the last one",
                        Tone::Info,
                    );
                    app.transcript.add_summary(&summary);
                    open = Some(OpenThread::new(thread_id));
                }
                Err(error) => app.error(format!("cannot fork: {error:#}")),
            }
        }
        // What waited for the turn goes once the agent is free.
        if !app.is_busy()
            && let Some(text) = app.release_queue()
        {
            send(client, &mut app, &mut open, text, false).await;
        }
        terminal_status.update(app.is_busy() || app.agents_at_work());
        // Work is activity too: the idle screen waits for both to stop.
        if app.is_busy() || app.agents_at_work() || update.is_some() {
            idle_since = Instant::now();
        }
        app.refresh_paths();
        app.update_available = app.status.available_update.clone();
        refresh_system(client, &mut app).await;
        refresh_jobs(client, &mut app).await;
        terminal.draw(|frame| ui::draw(frame, &mut app))?;
        let title = ui::tab_title(&app);
        if title != shown_title {
            crate::host::terminal::set_title(&title);
            shown_title = title;
        }
        let idle_after = Duration::from_secs(60 * u64::from(app.status.idle_screen_minutes));
        let action = tokio::select! {
            result = async {
                match &mut update {
                    Some(update) => update.result.as_mut().await,
                    None => std::future::pending().await,
                }
            } => {
                if let Some(update) = update.take() {
                    let success = result.is_ok();
                    let note = match result {
                        Ok(_) => format!("{} installed; start duskcode again to use it", update.tag),
                        Err(error) => format!("not updated: {error:#}"),
                    };
                    if let Some(Panel::Update { progress, .. }) = &mut app.panel {
                        *progress = success.then(|| client.status.borrow().install).flatten();
                    }
                    set_update_note(&mut app, note);
                }
                Action::None
            }
            key = keys.next() => match key {
                Some(event) => {
                    let event = event?;
                    let woke = wakes(&event);
                    if woke {
                        idle_since = Instant::now();
                    }
                    // The touch that ends the idle screen does nothing else.
                    if woke && app.screensaver.take().is_some() {
                        Action::None
                    } else {
                        app.handle(event)
                    }
                }
                None => Action::Quit,
            },
            _ = tokio::time::sleep_until((idle_since + idle_after).into()),
                if app.screensaver.is_none() && idle_after > Duration::ZERO => {
                app.screensaver = screensaver::Playing::random();
                // Without frames built in, look again after another idle spell.
                idle_since = Instant::now();
                Action::None
            }
            // The pressed shade is drawn away once its moment is over.
            _ = tokio::time::sleep(
                app.pressed.as_ref().map_or(PRESS, |pressed| PRESS.saturating_sub(pressed.at.elapsed())),
            ), if app.pressed.is_some() => Action::None,
            _ = tokio::time::sleep(app.screensaver.as_ref().map_or(FRAME, |playing| playing.frame_period())),
                if app.screensaver.is_some() => Action::None,
            Some(event) = client.events.recv() => {
                // Everything that has arrived meanwhile is drawn in one frame, so fast
                // streams do not cost a frame per token.
                let mut event = Some(event);
                for _ in 0..MAX_EVENTS_PER_FRAME {
                    let Some(next) = event else {
                        break;
                    };
                    show(&mut app, &mut open, next);
                    event = client.events.try_recv().ok();
                }
                Action::None
            }
            // The core changed something the screen shows, such as a model it found.
            Ok(()) = client.status.changed() => Action::None,
            // While the core looks for a model, the clock redraws what it finds.
            _ = system_clock.tick(), if system_open(&app) => Action::None,
            _ = jobs_clock.tick(), if jobs_wanted(&app) => Action::None,
            _ = memory_clock.tick() => {
                if let Ok(Some(usage)) = tokio::task::spawn_blocking(dusk_core::app::process_usage).await {
                    tracing::info!(
                        "memory {} MB, {} transcript entries",
                        usage.resident_bytes / (1024 * 1024),
                        app.transcript.entries.len()
                    );
                }
                Action::None
            }
            _ = git_clock.tick() => {
                app.git = crate::host::git::summary(&cwd).await;
                Action::None
            }
            _ = clock.tick(), if app.is_busy()
                || app.agents_at_work()
                || app.task_timers_run()
                || app.status.discovering_model
                || update.is_some()
                || mcp_starting => {
                app.tick();
                Action::None
            }
        };
        match apply(
            client,
            &mut app,
            &mut open,
            &mut terminal,
            &mut update,
            action,
        )
        .await?
        {
            Flow::Go => {}
            Flow::Quit => break,
        }
    }
    close(client, open.take()).await;
    Ok(())
}

/// Shows an event of the open thread; those of a thread closed meanwhile are dropped.
fn show(app: &mut App, open: &mut Option<OpenThread>, event: CoreEvent) {
    let Some(thread) = open else {
        return;
    };
    match event {
        CoreEvent::Thread { thread_id, event } if thread_id == thread.thread_id => {
            thread.apply(app, *event);
        }
        CoreEvent::Ended { thread_id } if thread_id == thread.thread_id => {
            thread.ended = true;
            app.error("the thread stopped; the next message opens it again");
            app.stop_work();
        }
        _ => {}
    }
}

/// A key, a click, a scroll or a paste counts as the user being back; a moving pointer
/// or a resize does not.
pub fn wakes(event: &Event) -> bool {
    match event {
        Event::Key(_) | Event::Paste(_) => true,
        Event::Mouse(mouse) => !matches!(mouse.kind, MouseEventKind::Moved),
        _ => false,
    }
}
