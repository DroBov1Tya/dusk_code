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
use dusk_core::{release::update::InstallProgress, runtime::Runtime};
use futures::StreamExt;
use ratatui::DefaultTerminal;

use super::{
    actions::{Flow, apply, set_update_note},
    answers::{jobs_wanted, refresh_jobs, refresh_system, system_open},
    thread::{OpenThread, close, next_event, resume, send},
};
use crate::{
    app::{Action, App, PRESS, Panel, SettingCategory, Tone},
    screensaver, ui,
};

pub const THREAD_SOURCE: &str = "tui";
/// How long a closing thread may take to stop its turn and finish.
pub const CLOSE_TIMEOUT: Duration = Duration::from_secs(10);
pub const FRAME: Duration = Duration::from_millis(80);
/// How often the git line is read again; edits made outside show within it.
pub const GIT_PERIOD: Duration = Duration::from_secs(5);
/// How often the system tab reads its numbers again.
pub const SYSTEM_PERIOD: Duration = Duration::from_secs(2);
/// How often the jobs are read again while one runs or their window is open.
pub const JOBS_PERIOD: Duration = Duration::from_secs(1);
pub const MAX_EVENTS_PER_FRAME: usize = 512;

pub struct InstallingUpdate<'a> {
    pub tag: String,
    pub progress: tokio::sync::watch::Receiver<InstallProgress>,
    pub result: Pin<Box<dyn Future<Output = Result<PathBuf>> + Send + 'a>>,
}

pub async fn run(
    runtime: &Runtime,
    mut terminal: DefaultTerminal,
    thread: Option<String>,
) -> Result<()> {
    let config = runtime.config().clone();
    let choice = runtime.model();
    let limit = runtime.input_limit();
    let mut app = App::new(choice.as_ref().map(|choice| choice.name()), limit, config);
    app.transcript
        .set_artifact_store(dusk_core::state::artifacts::ArtifactStore::new(
            runtime.paths().artifacts_dir(),
        ));
    let mut open = None;
    if let Some(thread_id) = thread {
        open = Some(resume(runtime, &mut app, &thread_id).await?);
    }
    // Shipped files follow the build; a local edit to one is replaced, so say which.
    let updated = runtime.updated_builtin();
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
    let cwd = env::current_dir()?;
    let mut shown_title = String::new();
    let mut terminal_status = crate::host::terminal::TerminalStatus::new();
    let mut known_model = choice.as_ref().map(|choice| choice.name());
    let mut update: Option<InstallingUpdate<'_>> = None;
    let mut idle_since = Instant::now();
    loop {
        if let Some(update) = &mut update
            && let Some(Panel::Update { progress, .. }) = &mut app.panel
        {
            *progress = Some(*update.progress.borrow_and_update());
        }
        if let Some(choice) = runtime.model() {
            let name = choice.name();
            // A saved model is not proof the provider answers, but one the core picked
            // after start was found over the network. No thread may exist yet to say so.
            if known_model.as_deref() != Some(name.as_str()) {
                app.provider_available = true;
                known_model = Some(name.clone());
            }
            app.model = name;
            app.limit = runtime.input_limit();
        }
        let config = runtime.config().current();
        app.effort = config.effort();
        app.mode = config.mode().to_owned();
        if let Some(Panel::Settings(view)) = &mut app.panel {
            view.selected_model = runtime.model().map(|choice| choice.name());
            view.context_policy = runtime.context_policy();
            view.effective_limit = runtime.input_limit();
            view.effort = app.effort;
        }
        // Read before the statuses: a server that settles in between still gets a frame.
        let mcp_starting = runtime.mcp_starting();
        app.mcp = runtime.mcp_statuses();
        if matches!(app.panel, Some(Panel::Plugins { .. })) {
            app.plugins = runtime
                .plugins()
                .into_iter()
                .map(|plugin| crate::app::PluginRow {
                    name: plugin.name,
                    version: plugin.version,
                    description: plugin.description,
                    enabled: plugin.enabled,
                    parts: plugin.parts,
                    note: plugin.problem,
                })
                .collect();
        }
        // Servers start in the background; the settings show them as they settle.
        if let Some(Panel::Settings(view)) = &mut app.panel
            && view.category == Some(SettingCategory::Mcp)
        {
            view.mcp = runtime.mcp_servers();
            if let Some(editor) = &mut view.editor {
                editor.update_mcp(&view.mcp);
            }
        }
        // /undo changed the history: it is read again, and the message goes back to the box.
        if let Some(text) = app.undone.take()
            && let Some(thread_id) = app.thread_id.clone()
        {
            match runtime.thread_messages(&thread_id).await {
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
            close(open.take()).await;
            match runtime.fork_thread(THREAD_SOURCE, &summary, &cwd).await {
                Ok(handle) => {
                    app.open(Some(handle.thread_id.clone()), None, &[]);
                    app.transcript.notice(
                        "forked: this thread goes on from a summary of the last one",
                        Tone::Info,
                    );
                    app.transcript.add_summary(&summary);
                    open = Some(OpenThread::new(handle));
                }
                Err(error) => app.error(format!("cannot fork: {error:#}")),
            }
        }
        // What waited for the turn goes once the agent is free.
        if !app.is_busy()
            && let Some(text) = app.release_queue()
        {
            send(runtime, &mut app, &mut open, text, false).await;
        }
        terminal_status.update(app.is_busy() || app.agents_at_work());
        // Work is activity too: the idle screen waits for both to stop.
        if app.is_busy() || app.agents_at_work() || update.is_some() {
            idle_since = Instant::now();
        }
        app.refresh_paths();
        app.update_available = runtime.available_update();
        refresh_system(runtime, &mut app).await;
        refresh_jobs(runtime, &mut app).await;
        terminal.draw(|frame| ui::draw(frame, &mut app))?;
        let title = ui::tab_title(&app);
        if title != shown_title {
            crate::host::terminal::set_title(&title);
            shown_title = title;
        }
        let idle_after =
            Duration::from_secs(60 * u64::from(config.appearance().idle_screen_minutes));
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
                        *progress = success.then(|| *update.progress.borrow());
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
            event = next_event(&mut open) => {
                match event {
                    Some(event) => {
                        // Everything that has arrived meanwhile is drawn in one frame, so
                        // fast streams do not cost a frame per token.
                        if let Some(thread) = &mut open {
                            thread.apply(&mut app, event);
                            for _ in 0..MAX_EVENTS_PER_FRAME {
                                let Some(event) = thread.handle.output.ready_event() else {
                                    break;
                                };
                                    thread.apply(&mut app, event);
                            }
                        }
                    }
                    None => {
                        app.error("the thread stopped; the next message opens it again");
                        app.stop_work();
                    }
                }
                Action::None
            }
            // While the core looks for a model, the clock redraws what it finds.
            _ = system_clock.tick(), if system_open(&app) => Action::None,
            _ = jobs_clock.tick(), if jobs_wanted(&app) => Action::None,
            _ = git_clock.tick() => {
                app.git = crate::host::git::summary(&cwd).await;
                Action::None
            }
            _ = clock.tick(), if app.is_busy()
                || app.agents_at_work()
                || app.task_timers_run()
                || runtime.discovering_model()
                || update.is_some()
                || mcp_starting => {
                app.tick();
                Action::None
            }
        };
        match apply(
            runtime,
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
    close(open.take()).await;
    Ok(())
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
