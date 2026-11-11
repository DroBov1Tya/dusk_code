//! The model the threads talk to, and its limits.

use crate::config::ContextPolicy;
use crate::runtime::ThreadEvent;
use crate::runtime::agent::Input;
use crate::runtime::{Runtime, model_choice::Choice};
use anyhow::{Context, Result};
use std::sync::Arc;

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
        let config = self.config().current();
        self.model()
            .map_or(crate::config::UNKNOWN_WINDOW, |choice| {
                choice.input_limit(&config)
            })
    }

    /// Whether the start is still looking for a model in the background.
    pub fn discovering_model(&self) -> bool {
        self.model_discovery
            .as_ref()
            .is_some_and(|task| !task.is_finished())
    }

    /// Switches every thread to `choice` from its next model call, and keeps it in the
    /// config.
    pub fn set_model(&self, choice: Choice) -> Result<()> {
        let name = choice.name();
        self.launcher
            .config
            .update(|config| config.set_model(name))?;
        self.launcher.model.set(choice.clone());
        let mailboxes = Arc::clone(&self.launcher.mailboxes);
        let input_limit = choice.input_limit(&self.launcher.config.current());
        let selected = choice;
        tokio::spawn(async move {
            for mailbox in mailboxes.lock().await.values() {
                mailbox.emit(ThreadEvent::ModelSelected {
                    name: selected.name(),
                    input_limit,
                });
                mailbox.send(Input::Sent);
            }
        });
        Ok(())
    }
}
