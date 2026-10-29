//! The loop that joins the screen and the core: keys, thread events and the spinner's
//! clock go in, the app changes, the screen is drawn again. A new thread is created only
//! when its first message is sent, so an opened and closed client leaves nothing behind.

use std::{env, time::Duration};

use anyhow::Result;
use crossterm::event::EventStream;
use dusk_core::runtime::{Runtime, ThreadEvent, ThreadHandle};
use futures::StreamExt;
use ratatui::DefaultTerminal;

use crate::{
    app::{Action, App, Panel, ThreadList},
    clipboard, ui,
};

const THREAD_SOURCE: &str = "tui";
/// How long a closing thread may take to stop its turn and finish.
const CLOSE_TIMEOUT: Duration = Duration::from_secs(10);
const FRAME: Duration = Duration::from_millis(80);

struct OpenThread {
    handle: ThreadHandle,
    /// The thread's task ended; the next message opens it again.
    ended: bool,
}

pub async fn run(
    runtime: &Runtime,
    mut terminal: DefaultTerminal,
    thread: Option<String>,
) -> Result<()> {
    let config = runtime.config().clone();
    let configured = config.current().limits().max_input_tokens;
    let model = runtime.model();
    let limit = model
        .context_window
        .map_or(configured, |window| window.min(configured));
    let skills_dir = runtime.paths().skills_dir();
    let mut app = App::new(
        model.id.clone(),
        limit,
        config,
        runtime.tool_names(),
        skills_dir,
    );
    let mut open = None;
    if let Some(thread_id) = thread {
        open = Some(resume(runtime, &mut app, &thread_id).await?);
    }
    let mut keys = EventStream::new();
    let mut clock = tokio::time::interval(FRAME);
    loop {
        terminal.draw(|frame| ui::draw(frame, &mut app))?;
        let action = tokio::select! {
            key = keys.next() => match key {
                Some(event) => app.handle(event?),
                None => Action::Quit,
            },
            event = next_event(&mut open) => {
                match event {
                    Some(event) => app.apply(event),
                    None => app.error("the thread stopped; the next message opens it again"),
                }
                Action::None
            }
            _ = clock.tick(), if app.is_busy() || app.subagents.iter().any(|agent| agent.finished_at.is_none()) => {
                app.tick();
                Action::None
            }
        };
        match action {
            Action::None => {}
            Action::Quit => break,
            Action::Send(text) => send(runtime, &mut app, &mut open, text, false).await,
            Action::SendNow(text) => send(runtime, &mut app, &mut open, text, true).await,
            Action::Cancel => {
                if let Some(thread) = &open
                    && let Err(error) = thread.handle.input.cancel()
                {
                    app.error(format!("{error:#}"));
                }
            }
            Action::ShowThreads => match runtime.list_threads().await {
                Ok(threads) => {
                    let current = app.thread_id.as_deref();
                    let selected = threads
                        .iter()
                        .position(|thread| Some(thread.id.as_str()) == current)
                        .unwrap_or(0);
                    app.panel = Some(Panel::Threads(ThreadList { threads, selected }));
                }
                Err(error) => app.error(format!("cannot list threads: {error:#}")),
            },
            Action::OpenThread(thread_id) if app.thread_id.as_deref() == Some(&thread_id) => {}
            Action::OpenThread(thread_id) => {
                close(open.take()).await;
                match resume(runtime, &mut app, &thread_id).await {
                    Ok(thread) => open = Some(thread),
                    Err(error) => {
                        app.open(None, None, &[]);
                        app.error(format!("cannot open the thread: {error:#}"));
                    }
                }
            }
            Action::NewThread => {
                close(open.take()).await;
                app.open(None, None, &[]);
            }
            Action::Copy(text) => {
                let lines = text.lines().count().max(1);
                app.flash = Some(if clipboard::copy(&text) {
                    format!("copied {lines} line{}", if lines == 1 { "" } else { "s" })
                } else {
                    "cannot copy: no clipboard tool".into()
                });
            }
        }
    }
    close(open.take()).await;
    Ok(())
}

/// The next event of the open thread; waits forever when none is open.
async fn next_event(open: &mut Option<OpenThread>) -> Option<ThreadEvent> {
    match open {
        Some(thread) if !thread.ended => {
            let event = thread.handle.output.next_event().await;
            thread.ended = event.is_none();
            event
        }
        _ => std::future::pending().await,
    }
}

async fn send(
    runtime: &Runtime,
    app: &mut App,
    open: &mut Option<OpenThread>,
    text: String,
    now: bool,
) {
    if let Err(error) = ensure_open(runtime, app, open).await {
        app.error(format!("cannot open a thread: {error:#}"));
        return;
    }
    let Some(thread) = open else {
        return;
    };
    let input = &thread.handle.input;
    let sent = if now {
        input.send_now(text).await
    } else {
        input.send(text).await
    };
    if let Err(error) = sent {
        app.error(format!("{error:#}"));
    }
}

/// Creates the thread on its first message, or reopens one whose task has ended.
async fn ensure_open(
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
            *open = Some(OpenThread {
                handle,
                ended: false,
            });
        }
        None => {
            let handle = runtime.new_thread(THREAD_SOURCE, &cwd).await?;
            app.thread_id = Some(handle.thread_id.clone());
            *open = Some(OpenThread {
                handle,
                ended: false,
            });
        }
    }
    Ok(())
}

async fn resume(runtime: &Runtime, app: &mut App, thread_id: &str) -> Result<OpenThread> {
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
    Ok(OpenThread {
        handle,
        ended: false,
    })
}

/// Stops the thread's turn and waits for its task to end, so it can be opened again.
async fn close(open: Option<OpenThread>) {
    let Some(OpenThread { handle, ended }) = open else {
        return;
    };
    let ThreadHandle {
        input, mut output, ..
    } = handle;
    if ended {
        return;
    }
    // Cancelling a thread that already ended has nothing to stop.
    let _ = input.cancel();
    drop(input);
    let drained = async { while output.next_event().await.is_some() {} };
    let _ = tokio::time::timeout(CLOSE_TIMEOUT, drained).await;
}
