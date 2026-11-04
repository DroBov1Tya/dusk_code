//! What each action of the app does with the core.

use std::env;

use anyhow::Result;
use dusk_core::{
    config::ApprovalPolicy,
    message::commands::Command,
    release::update::{Channel, InstallProgress},
    runtime::Runtime,
};
use ratatui::DefaultTerminal;

use super::{
    answers::{answer, preview},
    run::InstallingUpdate,
    thread::{OpenThread, close, ensure_open, resume, send},
};
use crate::{
    app::{Action, AgentWindow, App, FileEditor, ModelList, Panel, SwitchKind, Tone, Transcript},
    ui,
};

/// What the loop does next once an action is done.
pub enum Flow {
    Go,
    Quit,
}

/// Does what an action asks. Each arm is the whole of what one action means.
pub async fn apply<'a>(
    runtime: &'a Runtime,
    app: &mut App,
    open: &mut Option<OpenThread>,
    terminal: &mut DefaultTerminal,
    update: &mut Option<InstallingUpdate<'a>>,
    action: Action,
) -> Result<Flow> {
    match action {
        Action::None => {}
        Action::Quit if update.is_some() => {
            app.flash = Some("the update is still installing; wait before closing".into());
        }
        Action::Quit => return Ok(Flow::Quit),
        Action::TogglePlugin(name) => {
            let enabled = app
                .plugins
                .iter()
                .any(|plugin| plugin.name == name && plugin.enabled);
            if let Err(error) = runtime.set_plugin_enabled(&name, !enabled) {
                app.error(format!("cannot switch the plugin: {error:#}"));
            }
        }
        Action::SetPluginSecret { plugin, key, value } => {
            match runtime.set_plugin_secret(&plugin, &key, &value) {
                Ok(()) => {
                    if let Some(Panel::Plugins { entry, .. }) = &mut app.panel {
                        *entry = None;
                    }
                    app.flash = Some(format!("{key} set for this session only, not saved"));
                }
                Err(error) => {
                    if let Some(Panel::Plugins {
                        entry: Some(entry), ..
                    }) = &mut app.panel
                    {
                        entry.error = Some(format!("not kept: {error:#}"));
                    }
                }
            }
        }
        Action::Send(text) => send(runtime, app, open, text, false).await,
        Action::Approve { id, decision } => {
            if !runtime.answer_approval(id, decision) {
                app.flash = Some("that request is no longer waiting".into());
            }
        }
        Action::SendNow(text) => send(runtime, app, open, text, true).await,
        Action::Connect {
            kind,
            name,
            base_url,
            api_key,
        } => match runtime.connect_provider(kind, name, base_url, api_key) {
            Ok(_) => {
                app.panel = Some(Panel::Models(ModelList::checking(
                    runtime.model().map(|choice| choice.name()),
                )));
                terminal.draw(|frame| ui::draw(frame, app))?;
                answer(runtime, app, open, Command::Model { name: None }).await;
                app.flash =
                    Some("Provider saved; choose a model, then /resend for local messages".into());
            }
            Err(error) => {
                if let Some(Panel::Connect(form)) = &mut app.panel {
                    form.error = Some(format!("Could not save provider: {error:#}"));
                }
            }
        },
        Action::SetContextPolicy(policy) => match runtime.set_context_policy(policy) {
            Ok(limit) => {
                app.limit = limit;
                if let Some(Panel::Settings(view)) = &mut app.panel {
                    view.context_policy = Some(policy);
                    view.effective_limit = limit;
                    view.manual_context = None;
                    view.model_error = None;
                }
                app.flash = Some(format!(
                    "context saved for this model: {limit} tokens effective"
                ));
            }
            Err(error) => {
                if let Some(Panel::Settings(view)) = &mut app.panel {
                    view.model_error = Some(format!("not saved: {error:#}"));
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
        Action::SetIdleMinutes(minutes) => match runtime.set_idle_screen_minutes(minutes) {
            Ok(()) => {
                let minutes = runtime.config().current().appearance().idle_screen_minutes;
                if let Some(Panel::Settings(list)) = &mut app.panel {
                    list.idle_screen_minutes = minutes;
                }
                app.flash = Some(match minutes {
                    0 => "Idle screen: off".to_owned(),
                    minutes => format!("Idle screen: after {minutes} min"),
                });
            }
            Err(error) => app.error(format!("cannot save the idle screen: {error:#}")),
        },
        Action::SetGoalRounds(rounds) => match runtime.set_max_goal_rounds(rounds) {
            Ok(()) => {
                if let Some(Panel::Settings(list)) = &mut app.panel {
                    list.max_goal_rounds = rounds;
                }
                app.goal_rounds = rounds;
                app.flash = Some(format!("Goal rounds: {rounds}"));
            }
            Err(error) => app.error(format!("cannot save the goal rounds: {error:#}")),
        },
        Action::SetMemoryChars(chars) => match runtime.set_memory_chars(chars) {
            Ok(()) => {
                if let Some(Panel::Settings(list)) = &mut app.panel {
                    list.memory.max_chars = chars;
                }
                app.flash = Some(format!("Memory: up to {chars} characters"));
            }
            Err(error) => app.error(format!("cannot save the memory size: {error:#}")),
        },
        Action::SetWorkHistoryChars(chars) => match runtime.set_work_history_chars(chars) {
            Ok(()) => {
                let chars = runtime.config().current().work_history().max_chars;
                if let Some(Panel::Settings(list)) = &mut app.panel {
                    list.work_history.max_chars = chars;
                }
                app.flash = Some(format!("Project memory: up to {chars} characters"));
            }
            Err(error) => app.error(format!("cannot save the project memory size: {error:#}")),
        },
        Action::Switch { kind, name, on } => {
            let saved = match kind {
                SwitchKind::Agent => runtime.set_agent_enabled(&name, on),
                SwitchKind::Tool => runtime.set_tool_enabled(&name, on),
                SwitchKind::Skill => runtime.set_skill_enabled(&name, on),
                SwitchKind::Mcp => runtime.set_mcp_enabled(&name, on),
                SwitchKind::Memory => runtime.set_memory_enabled(on),
                SwitchKind::WorkHistory => runtime.set_work_history_enabled(on),
                SwitchKind::DevChannel => runtime.set_update_channel(match on {
                    true => Channel::Dev,
                    false => Channel::Stable,
                }),
                SwitchKind::Approvals(ApprovalPolicy::Sandbox)
                    if on && !runtime.platform().sandbox =>
                {
                    Err(anyhow::anyhow!("this system has no sandbox for commands"))
                }
                SwitchKind::Approvals(policy) => runtime.set_approvals(match on {
                    true => policy,
                    false => ApprovalPolicy::Ask,
                }),
                SwitchKind::Logging => runtime.set_logging_enabled(on),
            };
            match saved {
                Ok(()) => answer(runtime, app, open, Command::Settings).await,
                Err(error) => app.error(format!("cannot save the setting: {error:#}")),
            }
        }
        Action::OpenSkill(name) => match runtime.skill(&name) {
            Ok(skill) => open_editor(app, FileEditor::skill(skill), false),
            Err(error) => app.flash = Some(format!("cannot open the skill: {error:#}")),
        },
        Action::SaveSkill { name, text } => match runtime.save_skill(&name, &text) {
            Ok(skill) => {
                open_editor(app, FileEditor::skill(skill), true);
                app.flash = Some(format!("skill {name} saved"));
                answer(runtime, app, open, Command::Settings).await;
            }
            Err(error) => editor_error(app, format!("not saved: {error:#}")),
        },
        Action::OpenAgentFile(name) => match runtime.agent(&name) {
            Ok(agent) => open_editor(app, FileEditor::agent(agent), false),
            Err(error) => app.flash = Some(format!("cannot open the agent: {error:#}")),
        },
        Action::SaveAgentFile { name, text } => match runtime.save_agent(&name, &text).await {
            Ok(agent) => {
                open_editor(app, FileEditor::agent(agent), true);
                app.flash = Some(format!("agent {name} saved"));
                answer(runtime, app, open, Command::Settings).await;
            }
            Err(error) => editor_error(app, format!("not saved: {error:#}")),
        },
        Action::OpenMcp(name) => {
            let text = name.as_deref().map(|name| runtime.mcp_server_text(name));
            match text.transpose() {
                Ok(text) => {
                    let servers = runtime.mcp_servers();
                    let server = servers
                        .iter()
                        .find(|server| Some(&server.name) == name.as_ref());
                    open_editor(app, FileEditor::mcp(server, text), false);
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
                        open_editor(app, FileEditor::mcp(server, text), true);
                        app.flash = Some(format!("MCP server {name} saved, starting"));
                    }
                    None => {
                        if let Some(Panel::Settings(view)) = &mut app.panel {
                            view.editor = None;
                        }
                        app.flash = Some("MCP server removed".into());
                    }
                }
                answer(runtime, app, open, Command::Settings).await;
            }
            Err(error) => editor_error(app, format!("not saved: {error:#}")),
        },
        Action::SignInMcp(name) => match runtime.sign_in_mcp(&name).await {
            Ok(page) => {
                // The full address carries the sign-in's state; shown only when needed.
                let notice = match open_in_browser(page.as_str()) {
                    true => format!(
                        "Sign-in to {name} opened in the browser at {}",
                        page.origin().ascii_serialization()
                    ),
                    false => format!("Sign-in to {name}: open {page} in a browser"),
                };
                app.transcript.notice(notice, Tone::Info);
                app.flash = Some(format!("{name} starts again once you have signed in"));
            }
            Err(error) => app.flash = Some(format!("cannot sign in to {name}: {error:#}")),
        },
        Action::RestartMcp(name) => {
            runtime.restart_mcp(&name);
            app.flash = Some(format!("MCP server {name} starting again"));
            answer(runtime, app, open, Command::Settings).await;
        }
        Action::Resend(id) => {
            if runtime.model().is_none() {
                app.flash = Some("Connect a provider and choose a model before resending".into());
                return Ok(Flow::Go);
            }
            if let Err(error) = ensure_open(runtime, app, open).await {
                app.error(format!("cannot open the thread: {error:#}"));
                return Ok(Flow::Go);
            }
            if let Some(thread_id) = &app.thread_id {
                match runtime.resend_local_unsent(thread_id, &id).await {
                    Ok(()) => {
                        app.mark_local_sent(&id);
                        app.flash = Some("Local message queued".into());
                    }
                    Err(error) => app.error(format!("cannot resend the local message: {error:#}")),
                }
            }
        }
        Action::Cancel => {
            if let Some(thread) = &open
                && let Err(error) = thread.handle.input.cancel().await
            {
                app.error(format!("{error:#}"));
            }
        }
        Action::Command(Command::Undo { sequence, .. }) => {
            let thread_id = app.thread_id.clone();
            let command = Command::Undo {
                thread_id,
                sequence,
            };
            answer(runtime, app, open, command).await;
        }
        Action::Command(Command::Sources { add, .. }) => {
            // A file can be the first thing a chat gets.
            if add.is_some()
                && let Err(error) = ensure_open(runtime, app, open).await
            {
                app.error(format!("cannot open a thread: {error:#}"));
                return Ok(Flow::Go);
            }
            let thread_id = app.thread_id.clone();
            let command = Command::Sources { thread_id, add };
            answer(runtime, app, open, command).await;
        }
        Action::PinSource { id, pinned } => {
            if let Some(thread_id) = app.thread_id.clone() {
                if let Err(error) = runtime.pin_source(&thread_id, &id, pinned).await {
                    app.error(format!("cannot pin the source: {error:#}"));
                }
                let command = Command::Sources {
                    thread_id: Some(thread_id),
                    add: None,
                };
                answer(runtime, app, open, command).await;
            }
        }
        Action::RemoveSource(id) => {
            if let Some(thread_id) = app.thread_id.clone() {
                if let Err(error) = runtime.remove_source(&thread_id, &id).await {
                    app.error(format!("cannot remove the source: {error:#}"));
                }
                let command = Command::Sources {
                    thread_id: Some(thread_id),
                    add: None,
                };
                answer(runtime, app, open, command).await;
            }
        }
        Action::Command(Command::Jobs { .. }) => {
            let thread_id = app.thread_id.clone();
            answer(runtime, app, open, Command::Jobs { thread_id }).await;
        }
        Action::StopJob(job_id) => {
            if let Some(thread_id) = app.thread_id.clone() {
                match runtime.stop_job(&thread_id, &job_id).await {
                    Ok(_) => app.flash = Some("stopping the job".into()),
                    Err(error) => app.error(format!("cannot stop the job: {error:#}")),
                }
                app.jobs_stale = true;
            }
        }
        Action::Command(Command::Resume { .. }) => {
            let cwd = env::current_dir().ok();
            answer(runtime, app, open, Command::Resume { cwd }).await;
        }
        Action::Command(Command::Context { .. }) => {
            let thread_id = app.thread_id.clone();
            answer(runtime, app, open, Command::Context { thread_id }).await;
        }
        Action::Command(command) => {
            if let Command::Migrate { .. } = command {
                app.flash = Some("creating a private archive; backups are excluded".into());
                terminal.draw(|frame| ui::draw(frame, app))?;
            }
            // Checking providers takes a moment; the window says so meanwhile.
            if let Command::Update { .. } = command {
                let note = Some("asking GitHub for the newest version".into());
                app.panel = Some(Panel::Update {
                    info: None,
                    note,
                    progress: None,
                    button: None,
                });
                terminal.draw(|frame| ui::draw(frame, app))?;
            }
            if command == (Command::Model { name: None }) {
                app.panel = Some(Panel::Models(ModelList::checking(
                    runtime.model().map(|choice| choice.name()),
                )));
                terminal.draw(|frame| ui::draw(frame, app))?;
            }
            answer(runtime, app, open, command).await;
        }

        Action::InstallUpdate(tag) => {
            if update.is_none() {
                let (sender, progress) = tokio::sync::watch::channel(InstallProgress::default());
                let installing_tag = tag.clone();
                let result = Box::pin(async move {
                    runtime
                        .install_update_with_progress(&installing_tag, sender)
                        .await
                });
                *update = Some(InstallingUpdate {
                    tag,
                    progress,
                    result,
                });
                if let Some(Panel::Update {
                    progress,
                    note,
                    button,
                    ..
                }) = &mut app.panel
                {
                    *progress = Some(InstallProgress::default());
                    *note = None;
                    *button = None;
                }
            }
        }
        Action::OpenLink(url) => {
            app.flash = Some(match open_in_browser(&url) {
                true => format!("opened {url}"),
                false => format!("open {url} in a browser"),
            });
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
            match resume(runtime, app, &thread_id).await {
                Ok(thread) => *open = Some(thread),
                Err(error) => {
                    app.open(None, None, &[]);
                    app.error(format!("cannot open the thread: {error:#}"));
                }
            }
        }
        Action::Preview(thread_id) => preview(runtime, app, thread_id).await,
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
            app.flash = Some(if crate::host::clipboard::copy(&text) {
                format!("copied {lines} line{}", if lines == 1 { "" } else { "s" })
            } else {
                "cannot copy: no clipboard tool".into()
            });
        }
    }
    Ok(Flow::Go)
}

/// The system's browser shows the page; false when none could be started.
pub fn open_in_browser(url: &str) -> bool {
    let program = if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    std::process::Command::new(program)
        .arg(url)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .is_ok()
}

pub fn set_update_note(app: &mut App, text: String) {
    if let Some(Panel::Update { note, .. }) = &mut app.panel {
        *note = Some(text);
    }
}

/// Shows `editor` in the settings. After a save the file was read again and may differ,
/// so the cursor and scroll are kept, on a char boundary.
pub fn open_editor(app: &mut App, mut editor: FileEditor, keep_place: bool) {
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

pub fn editor_error(app: &mut App, error: String) {
    if let Some(Panel::Settings(view)) = &mut app.panel
        && let Some(editor) = &mut view.editor
    {
        editor.error = Some(error);
    }
}
