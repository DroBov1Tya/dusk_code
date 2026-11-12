//! What each action of the app does with the core.

use std::env;

use anyhow::Result;
use dusk_core::client::Client;
use dusk_protocol::{
    client::{Command, Switch},
    config::ApprovalPolicy,
    release::InstallProgress,
};
use ratatui::DefaultTerminal;

use super::{
    answers::{answer, preview},
    run::InstallingUpdate,
    thread::{OpenThread, close, ensure_open, send},
};
use crate::app::DiffView;
use crate::{
    app::{
        Action, AgentWindow, App, FileEditor, ModelList, Panel, PasteEditor, SwitchKind, Tone,
        Transcript,
    },
    ui,
};

/// What the loop does next once an action is done.
pub enum Flow {
    Go,
    Quit,
}

/// Does what an action asks. Each arm is the whole of what one action means.
pub async fn apply(
    client: &Client,
    app: &mut App,
    open: &mut Option<OpenThread>,
    terminal: &mut DefaultTerminal,
    update: &mut Option<InstallingUpdate>,
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
            if let Err(error) = client.settings().plugin_enabled(&name, !enabled).await {
                app.error(format!("cannot switch the plugin: {error:#}"));
            }
        }
        Action::SetPluginSecret { plugin, key, value } => {
            match client.settings().plugin_secret(&plugin, &key, &value).await {
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
        Action::Send(text) => send(client, app, open, text, false).await,
        Action::Approve { id, decision } => {
            if !matches!(client.threads().approve(id, decision).await, Ok(true)) {
                app.flash = Some("that request is no longer waiting".into());
            }
        }
        Action::SendNow(text) => send(client, app, open, text, true).await,
        Action::Connect {
            kind,
            name,
            base_url,
            api_key,
        } => match client
            .settings()
            .connect(kind, name, base_url, api_key)
            .await
        {
            Ok(_) => {
                let model = client.status.borrow().model.clone();
                app.panel = Some(Panel::Models(ModelList::checking(model)));
                terminal.draw(|frame| ui::draw(frame, app))?;
                answer(client, app, open, Command::Model { name: None }).await;
                app.flash =
                    Some("Provider saved; choose a model, then /resend for local messages".into());
            }
            Err(error) => {
                if let Some(Panel::Connect(form)) = &mut app.panel {
                    form.error = Some(format!("Could not save provider: {error:#}"));
                }
            }
        },
        Action::SetContextPolicy(policy) => match client.settings().context_policy(policy).await {
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
        Action::SetAgentLimit(limit) => match client.settings().max_agents_at_work(limit).await {
            Ok(()) => {
                if let Some(Panel::Settings(list)) = &mut app.panel {
                    list.max_agents_at_work = limit;
                }
                app.flash = Some(format!("Concurrent agents: {limit}"));
            }
            Err(error) => app.error(format!("cannot save the agent limit: {error:#}")),
        },
        Action::Schedule(action) => super::schedules::apply(client, app, action).await,
        Action::Disconnect(name) => match client.settings().remove_provider(&name).await {
            Ok(()) => {
                // An open model list shows the providers that are left.
                if matches!(app.panel, Some(Panel::Models(_))) {
                    let model = client.status.borrow().model.clone();
                    app.panel = Some(Panel::Models(ModelList::checking(model)));
                    terminal.draw(|frame| ui::draw(frame, app))?;
                    answer(client, app, open, Command::Model { name: None }).await;
                }
                app.flash = Some(format!("Provider {name} forgotten"));
            }
            Err(error) => app.error(format!("cannot forget the provider: {error:#}")),
        },
        Action::OpenMe => match client.catalog().profile().await {
            Ok(profile) => app.panel = Some(Panel::Paste(PasteEditor::me(profile.about))),
            Err(error) => app.error(format!("cannot read the profile: {error:#}")),
        },
        Action::SaveMe(text) => {
            let saved = async {
                let mut profile = client.catalog().profile().await?;
                profile.about = text;
                client.catalog().save_profile(profile).await
            }
            .await;
            match saved {
                Ok(()) => app.flash = Some("@me saved".to_owned()),
                Err(error) => app.error(format!("cannot save the profile: {error:#}")),
            }
        }
        Action::OpenSnippet(name) => match client.catalog().snippets().await {
            Ok(snippets) => {
                let text = snippets
                    .into_iter()
                    .find(|snippet| snippet.name == name)
                    .map(|snippet| snippet.text)
                    .unwrap_or_default();
                app.panel = Some(Panel::Paste(PasteEditor::snippet(name, text)));
            }
            Err(error) => app.error(format!("cannot read the snippets: {error:#}")),
        },
        Action::SaveSnippet(name, text) => {
            match client.catalog().save_snippet(&name, &text).await {
                Ok(()) => {
                    app.flash = Some(match text.trim().is_empty() {
                        true => format!("@{name} removed"),
                        false => format!("@{name} saved"),
                    })
                }
                Err(error) => app.error(format!("cannot save the snippet: {error:#}")),
            }
        }
        Action::OpenPlan => {
            let Some(thread_id) = app.thread_id.clone() else {
                app.flash = Some("Send a message first; the plan belongs to a chat".into());
                return Ok(Flow::Go);
            };
            match client.work().plan(&thread_id).await {
                Ok(plan) => {
                    let text = plan.unwrap_or_default();
                    app.panel = Some(Panel::Paste(PasteEditor::plan(text)));
                }
                Err(error) => app.error(format!("cannot read the plan: {error:#}")),
            }
        }
        Action::Diff => {
            let Some(thread_id) = app.thread_id.clone() else {
                app.flash = Some("Send a message first; the changes belong to a chat".into());
                return Ok(Flow::Go);
            };
            match client.work().diff(&thread_id, None).await {
                Ok(files) if files.is_empty() => {
                    app.flash = Some("No changes: the folder matches the last commit".into())
                }
                Ok(files) => {
                    app.panel = Some(Panel::Diff(DiffView {
                        files,
                        selected: 0,
                        open: false,
                        scroll: 0,
                    }))
                }
                Err(error) => app.error(format!("cannot read the changes: {error:#}")),
            }
        }
        Action::ChangedFiles => {
            let Some(thread_id) = app.thread_id.clone() else {
                app.flash = Some("Send a message first; the files belong to a chat".into());
                return Ok(Flow::Go);
            };
            match client.work().changed_files(&thread_id).await {
                Ok(files) if files.is_empty() => {
                    app.flash = Some("No commands of this chat changed a file yet".into())
                }
                Ok(files) => {
                    let shown: Vec<_> = files.iter().take(5).map(|f| f.path.as_str()).collect();
                    let more = files.len() - shown.len();
                    let tail = if more > 0 {
                        format!(" +{more}")
                    } else {
                        String::new()
                    };
                    app.flash = Some(format!("Changed: {}{tail}", shown.join(", ")));
                }
                Err(error) => app.error(format!("cannot read the files: {error:#}")),
            }
        }
        Action::SavePlan(text) => {
            if let Some(thread_id) = app.thread_id.clone() {
                match client.work().save_plan(&thread_id, &text).await {
                    Ok(()) => {
                        app.flash = Some(match text.trim().is_empty() {
                            true => "Plan removed".to_owned(),
                            false => "Plan saved; the model reads it with every message".to_owned(),
                        })
                    }
                    Err(error) => app.error(format!("cannot save the plan: {error:#}")),
                }
            }
        }
        Action::Access(name) => {
            let Some(thread_id) = app.thread_id.clone() else {
                app.flash = Some("Send a message first; the access mode belongs to a chat".into());
                return Ok(Flow::Go);
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
                return Ok(Flow::Go);
            };
            let policy = match name.as_str() {
                "default" => None,
                "ask" => Some(ApprovalPolicy::Ask),
                "sandbox" => Some(ApprovalPolicy::Sandbox),
                "arbiter" => Some(ApprovalPolicy::Arbiter),
                "full" | "full access" | "full_access" => Some(ApprovalPolicy::FullAccess),
                _ => {
                    app.flash = Some("Use ask, sandbox, arbiter, full or default".into());
                    return Ok(Flow::Go);
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
        Action::SetBranchPrefix(prefix) => match client.settings().branch_prefix(&prefix).await {
            Ok(()) => {
                if let Some(Panel::Settings(view)) = &mut app.panel {
                    view.branch_prefix = prefix.clone();
                    view.branch_input = None;
                }
                app.flash = Some(match prefix.is_empty() {
                    true => "Branch prefix: none".to_owned(),
                    false => format!("Branch prefix: {prefix}"),
                });
            }
            Err(error) => app.error(format!("cannot save the branch prefix: {error:#}")),
        },
        Action::SetIdleMinutes(minutes) => {
            match client.settings().idle_screen_minutes(minutes).await {
                Ok(minutes) => {
                    let minutes = minutes as u32;
                    if let Some(Panel::Settings(list)) = &mut app.panel {
                        list.idle_screen_minutes = minutes;
                    }
                    app.flash = Some(match minutes {
                        0 => "Idle screen: off".to_owned(),
                        minutes => format!("Idle screen: after {minutes} min"),
                    });
                }
                Err(error) => app.error(format!("cannot save the idle screen: {error:#}")),
            }
        }
        Action::SetGoalRounds(rounds) => match client.settings().max_goal_rounds(rounds).await {
            Ok(()) => {
                if let Some(Panel::Settings(list)) = &mut app.panel {
                    list.max_goal_rounds = rounds;
                }
                app.goal_rounds = rounds;
                app.flash = Some(format!("Goal rounds: {rounds}"));
            }
            Err(error) => app.error(format!("cannot save the goal rounds: {error:#}")),
        },
        Action::SetMemoryChars(chars) => match client.settings().memory_chars(chars).await {
            Ok(()) => {
                if let Some(Panel::Settings(list)) = &mut app.panel {
                    list.memory.max_chars = chars;
                }
                app.flash = Some(format!("Memory: up to {chars} characters"));
            }
            Err(error) => app.error(format!("cannot save the memory size: {error:#}")),
        },
        Action::SetWorkHistoryChars(chars) => {
            match client.settings().work_history_chars(chars).await {
                Ok(chars) => {
                    let chars = chars as usize;
                    if let Some(Panel::Settings(list)) = &mut app.panel {
                        list.work_history.max_chars = chars;
                    }
                    app.flash = Some(format!("Project memory: up to {chars} characters"));
                }
                Err(error) => app.error(format!("cannot save the project memory size: {error:#}")),
            }
        }
        Action::Switch { kind, name, on } => {
            let switch = match kind {
                SwitchKind::Agent => Switch::Agent(name),
                SwitchKind::Tool => Switch::Tool(name),
                SwitchKind::Skill => Switch::Skill(name),
                SwitchKind::Mcp => Switch::Mcp(name),
                SwitchKind::Memory => Switch::Memory,
                SwitchKind::WorkHistory => Switch::WorkHistory,
                SwitchKind::DevChannel => Switch::DevChannel,
                SwitchKind::Approvals(policy) => Switch::Approvals(policy),
                SwitchKind::Logging => Switch::Logging,
            };
            match client.settings().switch(switch, on).await {
                Ok(()) => answer(client, app, open, Command::Settings).await,
                Err(error) => app.error(format!("cannot save the setting: {error:#}")),
            }
        }
        Action::OpenSkill(name) => match client.catalog().skill(&name).await {
            Ok(skill) => open_editor(app, FileEditor::skill(skill), false),
            Err(error) => app.flash = Some(format!("cannot open the skill: {error:#}")),
        },
        Action::SaveSkill { name, text } => match client.catalog().save_skill(&name, &text).await {
            Ok(skill) => {
                open_editor(app, FileEditor::skill(skill), true);
                app.flash = Some(format!("skill {name} saved"));
                answer(client, app, open, Command::Settings).await;
            }
            Err(error) => editor_error(app, format!("not saved: {error:#}")),
        },
        Action::OpenAgentFile(name) => match client.catalog().agent(&name).await {
            Ok(agent) => open_editor(app, FileEditor::agent(agent), false),
            Err(error) => app.flash = Some(format!("cannot open the agent: {error:#}")),
        },
        Action::SaveAgentFile { name, text } => {
            match client.catalog().save_agent(&name, &text).await {
                Ok(agent) => {
                    open_editor(app, FileEditor::agent(agent), true);
                    app.flash = Some(format!("agent {name} saved"));
                    answer(client, app, open, Command::Settings).await;
                }
                Err(error) => editor_error(app, format!("not saved: {error:#}")),
            }
        }
        Action::OpenMcp(name) => {
            let text = match name.as_deref() {
                Some(name) => client.catalog().mcp_server_text(name).await.map(Some),
                None => Ok(None),
            };
            match text {
                Ok(text) => {
                    let servers = client.catalog().mcp_servers().await.unwrap_or_default();
                    let server = servers
                        .iter()
                        .find(|server| Some(&server.name) == name.as_ref());
                    open_editor(app, FileEditor::mcp(server, text), false);
                }
                Err(error) => app.flash = Some(format!("cannot open the server: {error:#}")),
            }
        }
        Action::SaveMcp { old, text } => match client
            .catalog()
            .save_mcp_server(old.as_deref(), &text)
            .await
        {
            Ok(saved) => {
                let servers = client.catalog().mcp_servers().await.unwrap_or_default();
                match &saved {
                    Some(name) => {
                        let server = servers.iter().find(|server| server.name == *name);
                        let text = client.catalog().mcp_server_text(name).await.ok();
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
                answer(client, app, open, Command::Settings).await;
            }
            Err(error) => editor_error(app, format!("not saved: {error:#}")),
        },
        Action::SignInMcp(name) => match client.catalog().sign_in_mcp(&name).await {
            Ok(page) => {
                // The full address carries the sign-in's state; shown only when needed.
                let notice = match open_in_browser(&page) {
                    true => format!(
                        "Sign-in to {name} opened in the browser at {}",
                        origin(&page)
                    ),
                    false => format!("Sign-in to {name}: open {page} in a browser"),
                };
                app.transcript.notice(notice, Tone::Info);
                app.flash = Some(format!("{name} starts again once you have signed in"));
            }
            Err(error) => app.flash = Some(format!("cannot sign in to {name}: {error:#}")),
        },
        Action::RestartMcp(name) => {
            if let Err(error) = client.catalog().restart_mcp(&name).await {
                app.error(format!("cannot restart the server: {error:#}"));
            }
            app.flash = Some(format!("MCP server {name} starting again"));
            answer(client, app, open, Command::Settings).await;
        }
        Action::Resend(id) => {
            if client.status.borrow().model.is_none() {
                app.flash = Some("Connect a provider and choose a model before resending".into());
                return Ok(Flow::Go);
            }
            if let Err(error) = ensure_open(client, app, open).await {
                app.error(format!("cannot open the thread: {error:#}"));
                return Ok(Flow::Go);
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
        Action::Cancel => {
            if let Some(thread) = &open
                && let Err(error) = client.threads().cancel(&thread.thread_id).await
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
            answer(client, app, open, command).await;
        }
        Action::Command(Command::Sources { add, .. }) => {
            // A file can be the first thing a chat gets.
            if add.is_some()
                && let Err(error) = ensure_open(client, app, open).await
            {
                app.error(format!("cannot open a thread: {error:#}"));
                return Ok(Flow::Go);
            }
            let thread_id = app.thread_id.clone();
            let command = Command::Sources { thread_id, add };
            answer(client, app, open, command).await;
        }
        Action::PinSource { id, pinned } => {
            if let Some(thread_id) = app.thread_id.clone() {
                if let Err(error) = client.work().pin_source(&thread_id, &id, pinned).await {
                    app.error(format!("cannot pin the source: {error:#}"));
                }
                let command = Command::Sources {
                    thread_id: Some(thread_id),
                    add: None,
                };
                answer(client, app, open, command).await;
            }
        }
        Action::RemoveSource(id) => {
            if let Some(thread_id) = app.thread_id.clone() {
                if let Err(error) = client.work().remove_source(&thread_id, &id).await {
                    app.error(format!("cannot remove the source: {error:#}"));
                }
                let command = Command::Sources {
                    thread_id: Some(thread_id),
                    add: None,
                };
                answer(client, app, open, command).await;
            }
        }
        Action::Command(Command::Jobs { .. }) => {
            let thread_id = app.thread_id.clone();
            answer(client, app, open, Command::Jobs { thread_id }).await;
        }
        Action::Background(call_id) => {
            if let Some(thread_id) = app.thread_id.clone() {
                match client
                    .threads()
                    .send_to_background(&thread_id, &call_id)
                    .await
                {
                    Ok(()) => app.flash = Some("the command goes on in the background".into()),
                    Err(error) => app.error(format!("cannot send it to the background: {error:#}")),
                }
                app.jobs_stale = true;
            }
        }
        Action::StopJob(job_id) => {
            if let Some(thread_id) = app.thread_id.clone() {
                match client.work().stop_job(&thread_id, &job_id).await {
                    Ok(_) => app.flash = Some("stopping the job".into()),
                    Err(error) => app.error(format!("cannot stop the job: {error:#}")),
                }
                app.jobs_stale = true;
            }
        }
        Action::Command(Command::Resume { .. }) => {
            let cwd = env::current_dir().ok();
            answer(client, app, open, Command::Resume { cwd }).await;
        }
        Action::Command(Command::Context { .. }) => {
            let thread_id = app.thread_id.clone();
            answer(client, app, open, Command::Context { thread_id }).await;
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
                let model = client.status.borrow().model.clone();
                app.panel = Some(Panel::Models(ModelList::checking(model)));
                terminal.draw(|frame| ui::draw(frame, app))?;
            }
            answer(client, app, open, command).await;
        }

        Action::InstallUpdate(tag) => {
            if update.is_none() {
                let result = Box::pin(client.install_update(&tag));
                *update = Some(InstallingUpdate { tag, result });
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
        Action::NextMode => match client.settings().next_mode().await {
            Ok(mode) => app.mode = mode,
            Err(error) => app.error(format!("cannot switch the mode: {error:#}")),
        },
        Action::StopAgent(thread_id) => match client.threads().stop_agent(&thread_id).await {
            Ok(()) => app.flash = Some("stopping the agent".into()),
            Err(error) => app.error(format!("cannot stop the agent: {error:#}")),
        },
        Action::OpenThread(thread_id) if app.thread_id.as_deref() == Some(&thread_id) => {}
        Action::OpenThread(thread_id) => {
            close(client, open.take()).await;
            match OpenThread::resume(client, app, &thread_id).await {
                Ok(thread) => *open = Some(thread),
                Err(error) => {
                    app.open(None, None, &[]);
                    app.error(format!("cannot open the thread: {error:#}"));
                }
            }
        }
        Action::Preview(thread_id) => preview(client, app, thread_id).await,
        Action::OpenAgent(thread_id) => match client.threads().messages(&thread_id).await {
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

/// The scheme and host of a page, without the path and query it carries.
fn origin(page: &str) -> String {
    page.splitn(4, '/').take(3).collect::<Vec<_>>().join("/")
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
