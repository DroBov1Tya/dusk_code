//! The settings a client changes.

use super::{Client, unexpected};
use anyhow::Result;
use dusk_protocol::{
    client::{Request, Response, Secret, SettingsRequest, Switch},
    config::{ContextPolicy, ProviderKind},
};

/// The calls that change `/settings`.
pub struct Settings<'a>(pub(super) &'a Client);

impl Settings<'_> {
    /// Returns the input limit the policy gives.
    pub async fn context_policy(&self, policy: ContextPolicy) -> Result<u64> {
        self.number(SettingsRequest::ContextPolicy(policy)).await
    }

    pub async fn max_agents_at_work(&self, limit: usize) -> Result<()> {
        self.done(SettingsRequest::MaxAgentsAtWork(limit)).await
    }

    /// Returns the minutes saved, after the config's bounds.
    pub async fn idle_screen_minutes(&self, minutes: u32) -> Result<u64> {
        self.number(SettingsRequest::IdleScreenMinutes(minutes))
            .await
    }

    pub async fn max_goal_rounds(&self, rounds: u32) -> Result<()> {
        self.done(SettingsRequest::MaxGoalRounds(rounds)).await
    }

    pub async fn memory_chars(&self, chars: usize) -> Result<()> {
        self.done(SettingsRequest::MemoryChars(chars)).await
    }

    /// Returns the characters saved, after the config's bounds.
    pub async fn work_history_chars(&self, chars: usize) -> Result<u64> {
        self.number(SettingsRequest::WorkHistoryChars(chars)).await
    }

    pub async fn switch(&self, switch: Switch, on: bool) -> Result<()> {
        self.done(SettingsRequest::Switch { switch, on }).await
    }

    /// The version control branch prefix; empty adds none.
    pub async fn branch_prefix(&self, prefix: &str) -> Result<()> {
        self.done(SettingsRequest::BranchPrefix(prefix.to_owned()))
            .await
    }

    /// Returns the mode switched to.
    pub async fn next_mode(&self) -> Result<String> {
        self.0
            .text(Request::Settings(SettingsRequest::NextMode))
            .await
    }

    pub async fn mode(&self, name: &str) -> Result<()> {
        self.done(SettingsRequest::Mode(name.to_owned())).await
    }

    /// Saves a provider and returns the name it was saved under.
    pub async fn connect(
        &self,
        kind: ProviderKind,
        name: Option<String>,
        base_url: String,
        api_key: Option<String>,
    ) -> Result<String> {
        let api_key = api_key.map(Secret);
        let request = SettingsRequest::Connect {
            kind,
            name,
            base_url,
            api_key,
        };
        self.0.text(Request::Settings(request)).await
    }

    /// Forgets a provider; refused while its model is the chosen one.
    pub async fn remove_provider(&self, name: &str) -> Result<()> {
        let name = name.to_owned();
        self.done(SettingsRequest::RemoveProvider { name }).await
    }

    /// Forgets every provider and the chosen model.
    pub async fn remove_all_providers(&self) -> Result<()> {
        self.done(SettingsRequest::RemoveAllProviders).await
    }

    /// Keeps the Hugging Face token in the system keychain.
    pub async fn set_secret(&self, name: &str, value: &str) -> Result<()> {
        let request = SettingsRequest::SetSecret {
            name: name.to_owned(),
            value: Secret(value.to_owned()),
        };
        self.done(request).await
    }

    /// Saves the speech servers; their keys go through `set_secret`.
    pub async fn set_voice(&self, voice: dusk_protocol::config::VoiceSettings) -> Result<()> {
        self.done(SettingsRequest::Voice(voice)).await
    }

    pub async fn delete_secret(&self, name: &str) -> Result<()> {
        let name = name.to_owned();
        self.done(SettingsRequest::DeleteSecret { name }).await
    }

    pub async fn plugin_enabled(&self, name: &str, enabled: bool) -> Result<()> {
        let name = name.to_owned();
        self.done(SettingsRequest::PluginEnabled { name, enabled })
            .await
    }

    /// Kept for this run only, never written to the config file.
    pub async fn plugin_secret(&self, plugin: &str, key: &str, value: &str) -> Result<()> {
        let (plugin, key) = (plugin.to_owned(), key.to_owned());
        let value = Secret(value.to_owned());
        self.done(SettingsRequest::PluginSecret { plugin, key, value })
            .await
    }

    async fn number(&self, request: SettingsRequest) -> Result<u64> {
        match self.0.call(Request::Settings(request)).await? {
            Response::Number(number) => Ok(number),
            other => Err(unexpected(other)),
        }
    }

    async fn done(&self, request: SettingsRequest) -> Result<()> {
        self.0.done(Request::Settings(request)).await
    }
}
