//! Requests that change the settings.

use super::Server;
use anyhow::{Result, ensure};
use dusk_protocol::{
    client::{Response, SettingsRequest, Switch},
    config::ApprovalPolicy,
    release::Channel,
};

impl Server {
    pub(super) async fn settings(&self, request: SettingsRequest) -> Result<Response> {
        let runtime = &self.runtime;
        match request {
            SettingsRequest::ContextPolicy(policy) => {
                return Ok(Response::Number(runtime.set_context_policy(policy)?));
            }
            SettingsRequest::MaxAgentsAtWork(limit) => runtime.set_max_agents_at_work(limit)?,
            SettingsRequest::IdleScreenMinutes(minutes) => {
                runtime.set_idle_screen_minutes(minutes)?;
                let saved = runtime.config().current().appearance().idle_screen_minutes;
                return Ok(Response::Number(saved.into()));
            }
            SettingsRequest::MaxGoalRounds(rounds) => runtime.set_max_goal_rounds(rounds)?,
            SettingsRequest::MemoryChars(chars) => runtime.set_memory_chars(chars)?,
            SettingsRequest::WorkHistoryChars(chars) => {
                runtime.set_work_history_chars(chars)?;
                let saved = runtime.config().current().work_history().max_chars;
                return Ok(Response::Number(saved as u64));
            }
            SettingsRequest::Switch { switch, on } => self.switch(switch, on)?,
            SettingsRequest::BranchPrefix(prefix) => runtime.set_branch_prefix(&prefix)?,
            SettingsRequest::NextMode => return Ok(Response::Text(runtime.next_mode()?)),
            SettingsRequest::Mode(name) => runtime.set_mode(&name)?,
            SettingsRequest::Connect {
                kind,
                name,
                base_url,
                api_key,
            } => {
                let api_key = api_key.map(|key| key.0);
                let name = runtime.connect_provider(kind, name, base_url, api_key)?;
                return Ok(Response::Text(name));
            }
            SettingsRequest::SignInChatGpt => {
                return Ok(Response::Text(runtime.sign_in_chatgpt().await?.to_string()));
            }
            SettingsRequest::CheckProvider(target) => {
                return Ok(Response::Provider(runtime.check_provider(target).await?));
            }
            SettingsRequest::ReconnectProvider { name, api_key } => {
                runtime.reconnect_provider(&name, api_key.map(|key| key.0))?;
                let target = dusk_protocol::client::ProviderTarget::Saved { name };
                return Ok(Response::Provider(runtime.check_provider(target).await?));
            }
            SettingsRequest::RemoveProvider { name } => runtime.remove_provider(&name)?,
            SettingsRequest::ProviderEnabled { name, enabled } => {
                runtime.set_provider_enabled(&name, enabled)?;
            }
            SettingsRequest::RemoveAllProviders => runtime.remove_all_providers()?,
            SettingsRequest::SetSecret { name, value } => {
                runtime.set_secret(&name, &value.0).await?
            }
            SettingsRequest::DeleteSecret { name } => runtime.delete_secret(&name).await?,
            SettingsRequest::Voice(voice) => runtime.set_voice(voice)?,
            SettingsRequest::PluginEnabled { name, enabled } => {
                runtime.set_plugin_enabled(&name, enabled)?;
            }
            SettingsRequest::PluginSecret { plugin, key, value } => {
                runtime.set_plugin_secret(&plugin, &key, &value.0)?;
            }
        }
        Ok(Response::Done)
    }

    pub(super) fn switch(&self, switch: Switch, on: bool) -> Result<()> {
        let runtime = &self.runtime;
        match switch {
            Switch::Agent(name) => runtime.set_agent_enabled(&name, on),
            Switch::Tool(name) => runtime.set_tool_enabled(&name, on),
            Switch::Skill(name) => runtime.set_skill_enabled(&name, on),
            Switch::Mcp(name) => runtime.set_mcp_enabled(&name, on),
            Switch::Memory => runtime.set_memory_enabled(on),
            Switch::WorkHistory => runtime.set_work_history_enabled(on),
            Switch::DevChannel => runtime.set_update_channel(match on {
                true => Channel::Dev,
                false => Channel::Stable,
            }),
            Switch::Approvals(ApprovalPolicy::Sandbox) if on => {
                ensure!(
                    runtime.platform().sandbox,
                    "this system has no sandbox for commands"
                );
                runtime.set_approvals(ApprovalPolicy::Sandbox)
            }
            Switch::Approvals(policy) => runtime.set_approvals(match on {
                true => policy,
                false => ApprovalPolicy::Ask,
            }),
            Switch::Logging => runtime.set_logging_enabled(on),
            Switch::Notifications => runtime.set_notifications(on),
            Switch::SoundNotifications => runtime.set_sound_notifications(on),
        }
    }
}
