//! The loop that joins the screen and the core: keys, thread events and the spinner's
//! clock go in, the app changes, the screen is drawn again. A new thread is created only
//! when its first message is sent, so an opened and closed client leaves nothing behind.

use std::{env, time::Duration};

use anyhow::Result;
use crossterm::event::EventStream;
use dusk_core::{
    chat::commands::{Answer, Command},
    config::Effort,
    config::{ApiKey, ProviderConfig, ProviderKind},
    providers::{Model, validate_provider},
    runtime::{Runtime, ThreadEvent, ThreadHandle},
    update::Channel,
};
use futures::StreamExt;
use ratatui::DefaultTerminal;

use crate::{
    app::{
        Action, AgentWindow, App, EFFORTS, FileEditor, ModelList, Panel, Preview, SettingCategory,
        SettingsView, SwitchKind, ThreadList, Tone, Transcript,
    },
    clipboard, git, ui,
};

const THREAD_SOURCE: &str = "tui";
/// How long a closing thread may take to stop its turn and finish.
const CLOSE_TIMEOUT: Duration = Duration::from_secs(10);
const FRAME: Duration = Duration::from_millis(80);
/// How often the git line is read again; edits made outside show within it.
const GIT_PERIOD: Duration = Duration::from_secs(5);
const MAX_EVENTS_PER_FRAME: usize = 512;

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
    let choice = runtime.model();
    let limit = choice.as_ref().map_or(
        runtime.config().current().limits().max_input_tokens,
        |choice| input_limit(runtime, &choice.model),
    );
    let mut app = App::new(choice.as_ref().map(|choice| choice.name()), limit, config);
    let mut open = None;
    if let Some(thread_id) = thread {
        open = Some(resume(runtime, &mut app, &thread_id).await?);
    }
    // Shipped prompts and skills the user changed stay theirs; the new ones wait beside.
    let kept = runtime.kept_builtin();
    if !kept.is_empty() {
        app.transcript.notice(
            format!(
                "dusk ships newer versions of files you changed: {}; yours are kept, the new \
                 ones are beside them as .new",
                kept.join(", ")
            ),
            Tone::Info,
        );
    }
    let mut keys = EventStream::new();
    let mut clock = tokio::time::interval(FRAME);
    let mut git_clock = tokio::time::interval(GIT_PERIOD);
    let cwd = env::current_dir()?;
    let mut shown_title = String::new();
    let mut known_model = choice.as_ref().map(|choice| choice.name());
    loop {
        if let Some(choice) = runtime.model() {
            let name = choice.name();
            // A saved model is not proof the provider answers, but one the core picked
            // after start was found over the network. No thread may exist yet to say so.
            if known_model.as_deref() != Some(name.as_str()) {
                app.provider_available = true;
                known_model = Some(name.clone());
            }
            app.model = name;
            app.limit = input_limit(runtime, &choice.model);
        }
        let config = runtime.config().current();
        app.effort = config.effort();
        app.mode = config.mode().to_owned();
        // Read before the statuses: a server that settles in between still gets a frame.
        let mcp_starting = runtime.mcp_starting();
        app.mcp = runtime.mcp_statuses();
        // Servers start in the background; the settings show them as they settle.
        if let Some(Panel::Settings(view)) = &mut app.panel
            && view.category == Some(SettingCategory::Mcp)
        {
            view.mcp = runtime.mcp_servers();
            if let Some(editor) = &mut view.editor {
                editor.update_mcp(&view.mcp);
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
                    open = Some(OpenThread {
                        handle,
                        ended: false,
                    });
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
        terminal.draw(|frame| ui::draw(frame, &mut app))?;
        let title = ui::tab_title(&app);
        if title != shown_title {
            ui::screen::set_title(&title);
            shown_title = title;
        }
        let action = tokio::select! {
            key = keys.next() => match key {
                Some(event) => app.handle(event?),
                None => Action::Quit,
            },
            event = next_event(&mut open) => {
                match event {
                    Some(event) => {
                        app.apply(event);
                        // Everything that has arrived meanwhile is drawn in one frame, so
                        // fast streams do not cost a frame per token.
                        if let Some(thread) = &mut open {
                            for _ in 0..MAX_EVENTS_PER_FRAME {
                                let Some(event) = thread.handle.output.ready_event() else {
                                    break;
                                };
                                app.apply(event);
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
            _ = git_clock.tick() => {
                app.git = git::summary(&cwd).await;
                Action::None
            }
            _ = clock.tick(), if app.is_busy()
                || app.agents_at_work()
                || app.task_timers_run()
                || runtime.discovering_model()
                || mcp_starting => {
                app.tick();
                Action::None
            }
        };
        match action {
            Action::None => {}
            Action::Quit => break,
            Action::Send(text) => send(runtime, &mut app, &mut open, text, false).await,
            Action::SendNow(text) => send(runtime, &mut app, &mut open, text, true).await,
            Action::Connect { base_url, api_key } => match connect(runtime, base_url, api_key) {
                Ok(()) => {
                    app.panel = Some(Panel::Models(ModelList::checking(
                        runtime.model().map(|choice| choice.name()),
                    )));
                    terminal.draw(|frame| ui::draw(frame, &mut app))?;
                    answer(runtime, &mut app, &mut open, Command::Model { name: None }).await;
                    app.flash = Some(
                        "Provider saved; choose a model, then /resend for local messages".into(),
                    );
                }
                Err(_) => {
                    if let Some(Panel::Connect(form)) = &mut app.panel {
                        form.error = Some(
                            "Could not save provider. Check the HTTPS URL and config access."
                                .into(),
                        );
                    }
                }
            },
            Action::SetAgentLimit(limit) => match runtime.set_max_agents_at_work(limit) {
                Ok(()) => {
                    if let Some(Panel::Settings(list)) = &mut app.panel {
                        list.max_agents_at_work = limit;
                    }
                    app.flash = Some(format!("Concurrent agents: {limit}"));
                }
                Err(error) => app.error(format!("cannot save the agent limit: {error:#}")),
            },
            Action::Switch { kind, name, on } => {
                let saved = match kind {
                    SwitchKind::Agent => runtime.set_agent_enabled(&name, on),
                    SwitchKind::Tool => runtime.set_tool_enabled(&name, on),
                    SwitchKind::Skill => runtime.set_skill_enabled(&name, on),
                    SwitchKind::Mcp => runtime.set_mcp_enabled(&name, on),
                    SwitchKind::Memory => runtime.set_memory_enabled(on),
                    SwitchKind::DevChannel => runtime.set_update_channel(match on {
                        true => Channel::Dev,
                        false => Channel::Stable,
                    }),
                };
                match saved {
                    Ok(()) => answer(runtime, &mut app, &mut open, Command::Settings).await,
                    Err(error) => app.error(format!("cannot save the setting: {error:#}")),
                }
            }
            Action::OpenSkill(name) => match runtime.skill(&name) {
                Ok(skill) => open_editor(&mut app, FileEditor::skill(skill), false),
                Err(error) => app.flash = Some(format!("cannot open the skill: {error:#}")),
            },
            Action::SaveSkill { name, text } => match runtime.save_skill(&name, &text) {
                Ok(skill) => {
                    open_editor(&mut app, FileEditor::skill(skill), true);
                    app.flash = Some(format!("skill {name} saved"));
                    answer(runtime, &mut app, &mut open, Command::Settings).await;
                }
                Err(error) => editor_error(&mut app, format!("not saved: {error:#}")),
            },
            Action::OpenAgentFile(name) => match runtime.agent(&name) {
                Ok(agent) => open_editor(&mut app, FileEditor::agent(agent), false),
                Err(error) => app.flash = Some(format!("cannot open the agent: {error:#}")),
            },
            Action::SaveAgentFile { name, text } => match runtime.save_agent(&name, &text).await {
                Ok(agent) => {
                    open_editor(&mut app, FileEditor::agent(agent), true);
                    app.flash = Some(format!("agent {name} saved"));
                    answer(runtime, &mut app, &mut open, Command::Settings).await;
                }
                Err(error) => editor_error(&mut app, format!("not saved: {error:#}")),
            },
            Action::OpenMcp(name) => {
                let text = name.as_deref().map(|name| runtime.mcp_server_text(name));
                match text.transpose() {
                    Ok(text) => {
                        let servers = runtime.mcp_servers();
                        let server = servers
                            .iter()
                            .find(|server| Some(&server.name) == name.as_ref());
                        open_editor(&mut app, FileEditor::mcp(server, text), false);
                    }
                    Err(error) => app.flash = Some(format!("cannot open the server: {error:#}")),
                }
            }
            Action::SaveMcp { old, text } => match runtime.save_mcp_server(old.as_deref(), &text) {
                Ok(saved) => {
                    let servers = runtime.mcp_servers();
                    match &saved {
                        Some(name) => {
                            let server = servers.iter().find(|server| server.name == *name);
                            let text = runtime.mcp_server_text(name).ok();
                            open_editor(&mut app, FileEditor::mcp(server, text), true);
                            app.flash = Some(format!("MCP server {name} saved, starting"));
                        }
                        None => {
                            if let Some(Panel::Settings(view)) = &mut app.panel {
                                view.editor = None;
                            }
                            app.flash = Some("MCP server removed".into());
                        }
                    }
                    answer(runtime, &mut app, &mut open, Command::Settings).await;
                }
                Err(error) => editor_error(&mut app, format!("not saved: {error:#}")),
            },
            Action::RestartMcp(name) => {
                runtime.restart_mcp(&name);
                app.flash = Some(format!("MCP server {name} starting again"));
                answer(runtime, &mut app, &mut open, Command::Settings).await;
            }
            Action::Resend(id) => {
                if runtime.model().is_none() {
                    app.flash =
                        Some("Connect a provider and choose a model before resending".into());
                    continue;
                }
                if let Err(error) = ensure_open(runtime, &mut app, &mut open).await {
                    app.error(format!("cannot open the thread: {error:#}"));
                    continue;
                }
                if let Some(thread_id) = &app.thread_id {
                    match runtime.resend_local_unsent(thread_id, &id).await {
                        Ok(()) => {
                            app.mark_local_sent(&id);
                            app.flash = Some("Local message queued".into());
                        }
                        Err(error) => {
                            app.error(format!("cannot resend the local message: {error:#}"))
                        }
                    }
                }
            }
            Action::Cancel => {
                if let Some(thread) = &open
                    && let Err(error) = thread.handle.input.cancel()
                {
                    app.error(format!("{error:#}"));
                }
            }
            Action::Command(Command::Context { .. }) => {
                let thread_id = app.thread_id.clone();
                answer(runtime, &mut app, &mut open, Command::Context { thread_id }).await;
            }
            Action::Command(command) => {
                // Checking providers takes a moment; the window says so meanwhile.
                if let Command::Update { .. } = command {
                    let note = Some("asking GitHub for the newest version".into());
                    app.panel = Some(Panel::Update {
                        info: None,
                        note,
                        button: None,
                    });
                    terminal.draw(|frame| ui::draw(frame, &mut app))?;
                }
                if command == (Command::Model { name: None }) {
                    app.panel = Some(Panel::Models(ModelList::checking(
                        runtime.model().map(|choice| choice.name()),
                    )));
                    terminal.draw(|frame| ui::draw(frame, &mut app))?;
                }
                answer(runtime, &mut app, &mut open, command).await;
            }

            Action::InstallUpdate(tag) => {
                set_update_note(&mut app, format!("downloading {tag}"));
                terminal.draw(|frame| ui::draw(frame, &mut app))?;
                let note = match runtime.install_update(&tag).await {
                    Ok(_) => format!("{tag} installed; start duskcode again to use it"),
                    Err(error) => format!("not updated: {error:#}"),
                };
                set_update_note(&mut app, note);
            }
            Action::NextMode => match runtime.next_mode() {
                Ok(mode) => app.mode = mode,
                Err(error) => app.error(format!("cannot switch the mode: {error:#}")),
            },
            Action::StopAgent(thread_id) => match runtime.stop_agent(&thread_id).await {
                Ok(()) => app.flash = Some("stopping the agent".into()),
                Err(error) => app.error(format!("cannot stop the agent: {error:#}")),
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
            Action::Preview(thread_id) => preview(runtime, &mut app, thread_id).await,
            Action::OpenAgent(thread_id) => match runtime.thread_messages(&thread_id).await {
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
            },
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

/// Allocates the internal name against the configuration read during the same update.
/// Existing providers remain intact even when their generated-looking names are occupied.
fn connect(runtime: &Runtime, base_url: String, api_key: Option<String>) -> Result<()> {
    let provider = ProviderConfig {
        kind: ProviderKind::OpenAiCompatible,
        base_url,
        api_key: ApiKey(api_key.unwrap_or_default()),
        inline_tags: false,
    };
    validate_provider(&provider)?;
    runtime.config().update(|config| {
        let mut index = 1usize;
        let mut name = format!("provider-{index}");
        while config.providers().contains_key(&name) {
            index += 1;
            name = format!("provider-{index}");
        }
        config.set_provider(name, provider);
    })?;
    Ok(())
}

/// Shows what the core answered to a command that does not run in the thread.
async fn answer(runtime: &Runtime, app: &mut App, open: &mut Option<OpenThread>, command: Command) {
    match command.answer(runtime).await {
        Ok(Answer::Threads(threads)) => {
            let current = app.thread_id.as_deref();
            let selected = threads
                .iter()
                .position(|thread| Some(thread.id.as_str()) == current)
                .unwrap_or(0);
            let first = threads.get(selected).map(|thread| thread.id.clone());
            app.panel = Some(Panel::Threads(ThreadList {
                threads,
                selected,
                preview: None,
            }));
            if let Some(thread_id) = first {
                preview(runtime, app, thread_id).await;
            }
        }
        Ok(Answer::Providers { current, providers }) => {
            app.panel = Some(Panel::Models(ModelList::checked(current, providers)));
        }
        Ok(Answer::ModelSwitched(choice)) => {
            app.panel = None;
            app.limit = input_limit(runtime, &choice.model);
            app.model = choice.name();
            app.provider_available = true;
            app.flash = Some(format!("model {}", choice.name()));
        }
        Ok(Answer::Settings(settings)) => match &mut app.panel {
            Some(Panel::Settings(view)) => view.refresh(settings),
            _ => app.panel = Some(Panel::Settings(SettingsView::new(settings))),
        },
        Ok(Answer::Effort { current, changed }) => {
            app.effort = current;
            let name = current.map_or("default", Effort::name);
            if changed {
                app.flash = Some(format!("effort {name}"));
            } else {
                let selected = EFFORTS.iter().position(|effort| *effort == name);
                app.panel = Some(Panel::Effort {
                    selected: selected.unwrap_or(0),
                });
            }
        }
        Ok(Answer::Update(info)) => {
            app.panel = Some(Panel::Update {
                info: Some(info),
                note: None,
                button: None,
            });
        }
        Ok(Answer::Context(report)) => {
            app.panel = Some(Panel::Context { report, row: 0 });
        }
        Ok(Answer::NewThread) => {
            close(open.take()).await;
            app.panel = None;
            app.open(None, None, &[]);
        }
        Err(error) => {
            app.panel = None;
            app.error(format!("{error:#}"));
        }
    }
}

/// The largest request: the configured input limit, or the model window when smaller.
fn input_limit(runtime: &Runtime, model: &Model) -> u64 {
    let configured = runtime.config().current().limits().max_input_tokens;
    model
        .context_window
        .map_or(configured, |window| window.min(configured))
}

/// Shows a thread in the threads panel, if the panel is still open. A preview that
/// cannot be read just stays empty; opening the thread reports the error.
async fn preview(runtime: &Runtime, app: &mut App, thread_id: String) {
    let Ok(history) = runtime.thread_messages(&thread_id).await else {
        return;
    };
    if let Some(Panel::Threads(list)) = &mut app.panel {
        list.preview = Some(Preview::new(thread_id, &history));
    }
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
        app.unsend(text);
        app.error(format!("cannot open a thread: {error:#}"));
        return;
    }
    let Some(thread) = open else {
        return;
    };
    let input = &thread.handle.input;
    let sent = if now {
        input.send_now(text.clone()).await
    } else {
        input.send(text.clone()).await
    };
    if let Err(error) = sent {
        app.unsend(text);
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
    app.tasks = runtime.thread_tasks(thread_id).await?;
    for message in runtime.local_unsent(thread_id).await? {
        app.show_local_unsent(message);
    }
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

fn set_update_note(app: &mut App, text: String) {
    if let Some(Panel::Update { note, .. }) = &mut app.panel {
        *note = Some(text);
    }
}

/// Shows `editor` in the settings. After a save the file was read again and may differ,
/// so the cursor and scroll are kept, on a char boundary.
fn open_editor(app: &mut App, mut editor: FileEditor, keep_place: bool) {
    let Some(Panel::Settings(view)) = &mut app.panel else {
        return;
    };
    if keep_place && let Some(old) = &view.editor {
        let mut cursor = old.text.cursor.min(editor.text.text.len());
        while !editor.text.text.is_char_boundary(cursor) {
            cursor -= 1;
        }
        editor.text.cursor = cursor;
        editor.scroll = old.scroll;
    }
    view.editor = Some(editor);
}

fn editor_error(app: &mut App, error: String) {
    if let Some(Panel::Settings(view)) = &mut app.panel
        && let Some(editor) = &mut view.editor
    {
        editor.error = Some(error);
    }
}
