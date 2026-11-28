//! The model the threads talk to, and its limits.

use crate::config::{ContextPolicy, Effort};
use crate::runtime::ThreadEvent;
use crate::runtime::{
    Runtime,
    model_choice::Choice,
    tiers::{self, Tier},
};
use anyhow::{Context, Result, ensure};
use dusk_protocol::config::{TierStates, is_model_name};

impl Runtime {
    /// The model threads talk to now, as `provider/model` and its details.
    pub fn model(&self) -> Option<Choice> {
        self.launcher.model.current()
    }

    /// The selected model's saved policy; None while no model is selected.
    pub fn context_policy(&self) -> Option<ContextPolicy> {
        let choice = self.model()?;
        Some(
            self.config()
                .current()
                .context_policy(&choice.provider_name, &choice.model.id),
        )
    }

    /// Saves a model-specific policy without contacting the provider. Returns its
    /// effective input limit, including the known model window when available.
    pub fn set_context_policy(&self, policy: ContextPolicy) -> Result<u64> {
        policy.validate()?;
        let choice = self
            .model()
            .context("select a model before changing its context")?;
        let config = self.config().update(|config| {
            config.set_context_policy(&choice.provider_name, &choice.model.id, policy);
        })?;
        Ok(choice.input_limit(&config))
    }

    pub fn input_limit(&self) -> u64 {
        self.launcher
            .model
            .current_input_limit(&self.config().current())
    }

    /// Whether the start is still looking for a model in the background.
    pub fn discovering_model(&self) -> bool {
        self.model_discovery
            .as_ref()
            .is_some_and(|task| !task.is_finished())
    }

    /// The model the chat `thread_id` answers on, or with none the one new chats start
    /// with, as `provider/model`.
    pub async fn model_of(&self, thread_id: Option<&str>) -> Result<Option<String>> {
        match thread_id {
            Some(thread_id) => Ok(self.of(thread_id).await.chat_model(thread_id).await?.name()),
            None => Ok(self.model().map(|choice| choice.name())),
        }
    }

    /// Switches the chat `thread_id` to `choice` from its next model call, with its agents
    /// and team from their next wake, and keeps it as the model new chats start with.
    /// Every other chat keeps its own; one that had none takes this one.
    pub async fn set_model(&self, thread_id: Option<&str>, choice: Choice) -> Result<()> {
        let name = choice.name();
        if let Some(thread_id) = thread_id {
            let launcher = self.of(thread_id).await;
            ensure!(
                launcher
                    .state
                    .load_thread(thread_id)
                    .await?
                    .is_some_and(|thread| thread.parent_thread_id.is_none()),
                "thread {thread_id} is not a chat"
            );
            launcher.state.set_chat_model(thread_id, &name).await?;
            if let Some(mailbox) = launcher.mailboxes.get(thread_id).await {
                mailbox.context.model.set(choice.clone());
            }
        }
        self.launcher
            .config
            .update(|config| config.set_model(name))?;
        self.launcher.model.set(choice.clone());
        self.launcher.model.fill_empty(&choice);
        // The mailboxes are shared with temporary chats, which this reaches too.
        self.launcher.announce_models().await;
        Ok(())
    }

    /// What tells a client that joins a running chat the model it answers on.
    pub async fn chat_model_event(&self, thread_id: &str) -> Result<Option<ThreadEvent>> {
        let chat = self.of(thread_id).await.chat_model(thread_id).await?;
        let config = self.config().current();
        Ok(chat.choice.map(|choice| ThreadEvent::ModelSelected {
            name: choice.name(),
            input_limit: choice.input_limit(&config),
            effort: chat.effort,
        }))
    }

    /// The effort of the chat `thread_id`, or with none the one new chats start with.
    pub async fn effort(&self, thread_id: Option<&str>) -> Result<Option<Effort>> {
        match thread_id {
            Some(thread_id) => Ok(self.of(thread_id).await.chat_model(thread_id).await?.effort),
            None => Ok(self.config().current().effort()),
        }
    }

    /// Sets the effort of the chat `thread_id` from its next model call, and keeps it as
    /// the one new chats start with; other chats keep theirs.
    pub async fn set_effort(&self, thread_id: Option<&str>, effort: Option<Effort>) -> Result<()> {
        if let Some(thread_id) = thread_id {
            let launcher = self.of(thread_id).await;
            ensure!(
                launcher
                    .state
                    .load_thread(thread_id)
                    .await?
                    .is_some_and(|thread| thread.parent_thread_id.is_none()),
                "thread {thread_id} is not a chat"
            );
            launcher.state.set_chat_effort(thread_id, effort).await?;
            if let Some(mailbox) = launcher.mailboxes.get(thread_id).await {
                mailbox.context.model.set_effort(Some(effort));
                if let Some(choice) = mailbox.context.model.current() {
                    mailbox.emit(ThreadEvent::ModelSelected {
                        name: choice.name(),
                        input_limit: choice.input_limit(&self.config().current()),
                        effort,
                    });
                }
            }
        }
        self.config().update(|config| config.set_effort(effort))?;
        Ok(())
    }

    /// Chooses the model of a subagent tier, `provider/model` of a connected provider;
    /// `None` unsets it, so subagents asked at it answer on their chat's model.
    pub fn set_model_tier(&self, tier: Tier, model: Option<String>) -> Result<()> {
        let model = model
            .map(|name| name.trim().to_owned())
            .filter(|name| !name.is_empty());
        if let Some(name) = &model {
            ensure!(is_model_name(name), "write the model as provider/model");
            ensure!(
                tiers::connected(&self.config().current(), name),
                "{name} names no connected provider"
            );
        }
        self.config()
            .update(|config| config.set_model_tier(tier, model))?;
        Ok(())
    }

    /// Each tier's model as the settings show it.
    pub fn model_tiers(&self) -> TierStates {
        tiers::states(&self.config().current())
    }

    /// A catalogue just read may know the window of a chosen model the start missed.
    pub fn learn_model(&self, statuses: &[dusk_protocol::model::ProviderStatus]) {
        if !self.launcher.model.learn(statuses) {
            return;
        }
        let launcher = self.launcher.clone();
        tokio::spawn(async move { launcher.announce_models().await });
    }
}
