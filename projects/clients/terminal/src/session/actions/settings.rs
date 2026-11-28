//! Actions on providers, plugins and the values of the settings panel.

use anyhow::Result;
use dusk_core::client::Client;
use dusk_protocol::{
    client::{Command, Switch},
    config::{ContextPolicy, ModelTier, ProviderKind},
};
use ratatui::DefaultTerminal;

use crate::app::{App, ModelList, Panel, SwitchKind};
use crate::session::{answers::answer, thread::OpenThread};
use crate::ui;

pub async fn toggle_plugin(client: &Client, app: &mut App, name: String) {
    let enabled = app
        .plugins
        .iter()
        .any(|plugin| plugin.name == name && plugin.enabled);
    if let Err(error) = client.settings().plugin_enabled(&name, !enabled).await {
        app.error(format!("cannot switch the plugin: {error:#}"));
    }
}

pub async fn set_plugin_secret(
    client: &Client,
    app: &mut App,
    plugin: String,
    key: String,
    value: String,
) {
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

// The fields of `Action::Connect`, passed on as they are.
#[allow(clippy::too_many_arguments)]
pub async fn connect(
    client: &Client,
    app: &mut App,
    open: &mut Option<OpenThread>,
    terminal: &mut DefaultTerminal,
    kind: ProviderKind,
    name: Option<String>,
    base_url: String,
    api_key: Option<String>,
) -> Result<()> {
    match client
        .settings()
        .connect(kind, name, base_url, api_key)
        .await
    {
        Ok(_) => {
            let model = client.status.borrow().model.clone();
            app.panel = Some(Panel::Models(ModelList::checking(model)));
            terminal.draw(|frame| ui::draw(frame, app))?;
            let thread_id = app.thread_id.clone();
            let models = Command::Model {
                name: None,
                thread_id,
            };
            answer(client, app, open, models).await;
            app.flash =
                Some("Provider saved; choose a model, then /resend for local messages".into());
        }
        Err(error) => {
            if let Some(Panel::Connect(form)) = &mut app.panel {
                form.error = Some(format!("Could not save provider: {error:#}"));
            }
        }
    }
    Ok(())
}

pub async fn disconnect(
    client: &Client,
    app: &mut App,
    open: &mut Option<OpenThread>,
    terminal: &mut DefaultTerminal,
    name: String,
) -> Result<()> {
    match client.settings().remove_provider(&name).await {
        Ok(()) => {
            // An open model list shows the providers that are left.
            if matches!(app.panel, Some(Panel::Models(_))) {
                let model = client.status.borrow().model.clone();
                app.panel = Some(Panel::Models(ModelList::checking(model)));
                terminal.draw(|frame| ui::draw(frame, app))?;
                let thread_id = app.thread_id.clone();
                let models = Command::Model {
                    name: None,
                    thread_id,
                };
                answer(client, app, open, models).await;
            }
            app.flash = Some(format!("Provider {name} forgotten"));
        }
        Err(error) => app.error(format!("cannot forget the provider: {error:#}")),
    }
    Ok(())
}

pub async fn provider_enabled(
    client: &Client,
    app: &mut App,
    open: &mut Option<OpenThread>,
    terminal: &mut DefaultTerminal,
    name: String,
    on: bool,
) -> Result<()> {
    match client.settings().provider_enabled(&name, on).await {
        Ok(()) => {
            let model = client.status.borrow().model.clone();
            app.panel = Some(Panel::Models(ModelList::checking(model)));
            terminal.draw(|frame| ui::draw(frame, app))?;
            let thread_id = app.thread_id.clone();
            let models = Command::Model {
                name: None,
                thread_id,
            };
            answer(client, app, open, models).await;
            let state = if on { "on" } else { "off" };
            app.flash = Some(format!("Provider {name} is {state}"));
        }
        Err(error) => app.error(format!("cannot switch the provider: {error:#}")),
    }
    Ok(())
}

pub async fn set_context_policy(client: &Client, app: &mut App, policy: ContextPolicy) {
    match client.settings().context_policy(policy).await {
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
    }
}

pub async fn set_agent_limit(client: &Client, app: &mut App, limit: usize) {
    match client.settings().max_agents_at_work(limit).await {
        Ok(()) => {
            if let Some(Panel::Settings(list)) = &mut app.panel {
                list.max_agents_at_work = limit;
            }
            app.flash = Some(format!("Concurrent agents: {limit}"));
        }
        Err(error) => app.error(format!("cannot save the agent limit: {error:#}")),
    }
}

pub async fn set_branch_prefix(client: &Client, app: &mut App, prefix: String) {
    match client.settings().branch_prefix(&prefix).await {
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
    }
}

/// `/tier`: sets a subagent tier's model, then shows every tier as the status has it.
pub async fn tier(client: &Client, app: &mut App, ask: Option<(ModelTier, Option<String>)>) {
    if let Some((tier, model)) = ask {
        match client.settings().model_tier(tier, model.as_deref()).await {
            Ok(()) => {
                let shown = model.as_deref().unwrap_or("the chat's model");
                app.flash = Some(format!("{} tier: {shown}", tier.name()));
            }
            Err(error) => app.error(format!("cannot set the {} tier: {error:#}", tier.name())),
        }
        return;
    }
    let tiers = client.status.borrow().model_tiers.clone();
    let lines: Vec<String> = ModelTier::ALL
        .into_iter()
        .map(|tier| {
            let state = tiers.get(tier);
            let model = state.chosen.as_deref().unwrap_or("the chat's model");
            let missing = if state.missing {
                " (provider not connected: the chat's model answers)"
            } else {
                ""
            };
            format!("{}: {model}{missing}", tier.name())
        })
        .collect();
    app.transcript.notice(
        format!("Subagent tiers\n{}", lines.join("\n")),
        crate::app::Tone::Info,
    );
}

pub async fn set_idle_minutes(client: &Client, app: &mut App, minutes: u32) {
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

pub async fn set_model_timeout(client: &Client, app: &mut App, seconds: u32) {
    match client.settings().model_timeout(seconds).await {
        Ok(()) => {
            if let Some(Panel::Settings(list)) = &mut app.panel {
                list.model_timeout_seconds = seconds;
            }
            app.flash = Some(format!("Model timeout: {seconds} seconds of silence"));
        }
        Err(error) => app.error(format!("cannot save the model timeout: {error:#}")),
    }
}

pub async fn set_auto_compact_percent(client: &Client, app: &mut App, percent: u64) {
    match client.settings().auto_compact_percent(percent).await {
        Ok(saved) => {
            if let Some(Panel::Settings(list)) = &mut app.panel {
                list.auto_compact_percent = saved;
            }
            app.flash = Some(format!("Compact at {saved}% of the context"));
        }
        Err(error) => app.error(format!("cannot save where compaction starts: {error:#}")),
    }
}

pub async fn set_max_attempts(client: &Client, app: &mut App, attempts: u32) {
    match client.settings().max_attempts(attempts).await {
        Ok(()) => {
            if let Some(Panel::Settings(list)) = &mut app.panel {
                list.max_attempts = attempts;
            }
            app.flash = Some(format!("Tries per call: {attempts}"));
        }
        Err(error) => app.error(format!("cannot save the tries per call: {error:#}")),
    }
}

pub async fn set_memory_chars(client: &Client, app: &mut App, chars: usize) {
    match client.settings().memory_chars(chars).await {
        Ok(()) => {
            if let Some(Panel::Settings(list)) = &mut app.panel {
                list.memory.max_chars = chars;
            }
            app.flash = Some(format!("Memory: up to {chars} characters"));
        }
        Err(error) => app.error(format!("cannot save the memory size: {error:#}")),
    }
}

pub async fn set_work_history_chars(client: &Client, app: &mut App, chars: usize) {
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

pub async fn switch(
    client: &Client,
    app: &mut App,
    open: &mut Option<OpenThread>,
    kind: SwitchKind,
    name: String,
    on: bool,
) {
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
        SwitchKind::AutoCompact => Switch::AutoCompact,
    };
    match client.settings().switch(switch, on).await {
        Ok(()) => answer(client, app, open, Command::Settings).await,
        Err(error) => app.error(format!("cannot save the setting: {error:#}")),
    }
}

pub async fn restart_mcp(
    client: &Client,
    app: &mut App,
    open: &mut Option<OpenThread>,
    name: String,
) {
    if let Err(error) = client.catalog().restart_mcp(&name).await {
        app.error(format!("cannot restart the server: {error:#}"));
    }
    app.flash = Some(format!("MCP server {name} starting again"));
    answer(client, app, open, Command::Settings).await;
}

pub async fn next_mode(client: &Client, app: &mut App) {
    match client.settings().next_mode().await {
        Ok(mode) => app.mode = mode,
        Err(error) => app.error(format!("cannot switch the mode: {error:#}")),
    }
}
