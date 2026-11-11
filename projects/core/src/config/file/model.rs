//! The chosen model, how hard it reasons, and what its context may hold.

use super::Config;
use crate::config::{ContextPolicy, Effort};

impl Config {
    pub fn model(&self) -> Option<&str> {
        self.model.as_deref()
    }

    pub fn set_model(&mut self, name: String) {
        self.model = Some(name);
    }

    pub fn effort(&self) -> Option<Effort> {
        self.effort
    }

    pub fn set_effort(&mut self, effort: Option<Effort>) {
        if self.effort == effort {
            return;
        }
        self.previous_effort = if effort == Some(Effort::Off) {
            self.effort
        } else {
            None
        };
        self.effort = effort;
        self.effort_revision = self.effort_revision.wrapping_add(1);
    }

    /// A delayed rejection must not overwrite an effort selected after its request.
    pub(crate) fn restore_unsupported_effort(&mut self, requested: &Config) -> bool {
        if requested.effort != Some(Effort::Off)
            || self.effort != Some(Effort::Off)
            || self.effort_revision != requested.effort_revision
        {
            return false;
        }
        self.set_effort(self.previous_effort);
        true
    }

    pub fn context_policy(&self, provider: &str, model: &str) -> ContextPolicy {
        self.model_contexts
            .get(&format!("{provider}/{model}"))
            .copied()
            .unwrap_or_default()
    }

    pub fn set_context_policy(&mut self, provider: &str, model: &str, policy: ContextPolicy) {
        let name = format!("{provider}/{model}");
        if policy == ContextPolicy::Auto {
            self.model_contexts.remove(&name);
        } else {
            self.model_contexts.insert(name, policy);
        }
    }

    pub fn input_limit(&self, provider: &str, model: &str, window: Option<u64>) -> u64 {
        self.context_policy(provider, model).input_limit(window)
    }
}
