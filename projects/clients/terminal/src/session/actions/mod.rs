//! What each action of the app does with the core. `apply` is the one dispatch; the work
//! of each action lives in the file of its area:
//!
//! - `thread`: the thread on screen, its turns, agents and access mode;
//! - `work`: what a thread works on: plan, changes, sources, jobs;
//! - `settings`: providers, plugins and the values of the settings panel;
//! - `files`: the files the app edits in place: @me, snippets, skills, agents, MCP servers;
//! - `panels`: commands answered in a panel, and the update window;
//! - `browser`: pages opened in the system's browser.

mod browser;
mod files;
mod panels;
mod settings;
mod thread;
mod work;

pub use panels::set_update_note;

use anyhow::Result;
use dusk_core::client::Client;
use dusk_protocol::client::Command;
use ratatui::DefaultTerminal;

use super::{
    answers::preview,
    run::InstallingUpdate,
    thread::{OpenThread, send},
};
use crate::app::{Action, App};

/// What the loop does next once an action is done.
pub enum Flow {
    Go,
    Quit,
}

/// Does what an action asks. Each arm hands one action to the function that is the whole
/// of what it means.
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
        Action::TogglePlugin(name) => settings::toggle_plugin(client, app, name).await,
        Action::SetPluginSecret { plugin, key, value } => {
            settings::set_plugin_secret(client, app, plugin, key, value).await
        }
        Action::Send(text) => send(client, app, open, text, false).await,
        Action::Approve { id, decision } => thread::approve(client, app, id, decision).await,
        Action::SendNow(text) => send(client, app, open, text, true).await,
        Action::Connect {
            kind,
            name,
            base_url,
            api_key,
        } => settings::connect(client, app, open, terminal, kind, name, base_url, api_key).await?,
        Action::SetContextPolicy(policy) => settings::set_context_policy(client, app, policy).await,
        Action::SetAgentLimit(limit) => settings::set_agent_limit(client, app, limit).await,
        Action::Schedule(action) => super::schedules::apply(client, app, action).await,
        Action::Disconnect(name) => settings::disconnect(client, app, open, terminal, name).await?,
        Action::ProviderEnabled(name, on) => {
            settings::provider_enabled(client, app, open, terminal, name, on).await?
        }
        Action::OpenMe => files::open_me(client, app).await,
        Action::SaveMe(text) => files::save_me(client, app, text).await,
        Action::OpenSnippet(name) => files::open_snippet(client, app, name).await,
        Action::SaveSnippet(name, text) => files::save_snippet(client, app, name, text).await,
        Action::OpenPlan => work::open_plan(client, app).await,
        Action::Diff => work::diff(client, app).await,
        Action::ChangedFiles => work::changed_files(client, app).await,
        Action::SavePlan(text) => work::save_plan(client, app, text).await,
        Action::Access(name) => thread::access(client, app, name).await,
        Action::SetBranchPrefix(prefix) => settings::set_branch_prefix(client, app, prefix).await,
        Action::SetIdleMinutes(minutes) => settings::set_idle_minutes(client, app, minutes).await,
        Action::SetGoalRounds(rounds) => settings::set_goal_rounds(client, app, rounds).await,
        Action::SetMemoryChars(chars) => settings::set_memory_chars(client, app, chars).await,
        Action::SetWorkHistoryChars(chars) => {
            settings::set_work_history_chars(client, app, chars).await
        }
        Action::Switch { kind, name, on } => {
            settings::switch(client, app, open, kind, name, on).await
        }
        Action::OpenSkill(name) => files::open_skill(client, app, name).await,
        Action::SaveSkill { name, text } => files::save_skill(client, app, open, name, text).await,
        Action::OpenAgentFile(name) => files::open_agent_file(client, app, name).await,
        Action::SaveAgentFile { name, text } => {
            files::save_agent_file(client, app, open, name, text).await
        }
        Action::OpenMcp(name) => files::open_mcp(client, app, name).await,
        Action::SaveMcp { old, text } => files::save_mcp(client, app, open, old, text).await,
        Action::SignInChatGpt => browser::sign_in_chatgpt(client, app).await,
        Action::SignInMcp(name) => browser::sign_in_mcp(client, app, name).await,
        Action::RestartMcp(name) => settings::restart_mcp(client, app, open, name).await,
        Action::Resend(id) => thread::resend(client, app, open, id).await,
        Action::Cancel => thread::cancel(client, app, open).await,
        Action::Command(Command::Undo { sequence, .. }) => {
            thread::undo(client, app, open, sequence).await
        }
        Action::Command(Command::Sources { add, .. }) => {
            work::sources(client, app, open, add).await
        }
        Action::PinSource { id, pinned } => work::pin_source(client, app, open, id, pinned).await,
        Action::RemoveSource(id) => work::remove_source(client, app, open, id).await,
        Action::Command(Command::Jobs { .. }) => work::jobs(client, app, open).await,
        Action::Background(call_id) => work::background(client, app, call_id).await,
        Action::StopJob(job_id) => work::stop_job(client, app, job_id).await,
        Action::Command(Command::Resume { .. }) => thread::resume(client, app, open).await,
        Action::Command(Command::Context { .. }) => thread::context(client, app, open).await,
        Action::Command(command) => panels::command(client, app, open, terminal, command).await?,
        Action::InstallUpdate(tag) => panels::install_update(client, app, update, tag),
        Action::OpenLink(url) => browser::open_link(app, url),
        Action::NextMode => settings::next_mode(client, app).await,
        Action::StopAgent(thread_id) => thread::stop_agent(client, app, thread_id).await,
        Action::OpenThread(thread_id) if app.thread_id.as_deref() == Some(&thread_id) => {}
        Action::OpenThread(thread_id) => thread::open_thread(client, app, open, thread_id).await,
        Action::Preview(thread_id) => preview(client, app, thread_id).await,
        Action::OpenAgent(thread_id) => thread::open_agent(client, app, thread_id).await,
        Action::Copy(text) => copy(app, text),
    }
    Ok(Flow::Go)
}

fn copy(app: &mut App, text: String) {
    let lines = text.lines().count().max(1);
    app.flash = Some(if crate::host::clipboard::copy(&text) {
        format!("copied {lines} line{}", if lines == 1 { "" } else { "s" })
    } else {
        "cannot copy: no clipboard tool".into()
    });
}
