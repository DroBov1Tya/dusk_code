//! How the arbiter of auto mode judges: the model it asks and the review prompt it reads.

use serde::{Deserialize, Serialize};

/// The longest review prompt the user may write, in characters.
pub const MAX_ARBITER_PROMPT_CHARS: usize = 8_000;

/// The arbiter's settings. The prompt only guides its judgment: the core still asks the
/// user when the answer is unclear and never lets it allow what the approval mode forbids.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Arbiter {
    /// `provider/model`; `None` judges on the chat's model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// The user's review prompt; `None` keeps the built-in one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
}

impl Arbiter {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    /// Empty text means the default, so a cleared field resets it.
    pub fn normalized(self) -> Self {
        let keep = |text: Option<String>| {
            text.map(|text| text.trim().to_owned())
                .filter(|text| !text.is_empty())
        };
        Self {
            model: keep(self.model),
            prompt: keep(self.prompt),
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if let Some(model) = &self.model {
            let named = model.split_once('/').is_some_and(|(provider, model)| {
                !provider.trim().is_empty() && !model.trim().is_empty()
            });
            if !named || model.chars().any(char::is_control) {
                return Err("arbiter.model must be written as provider/model".into());
            }
        }
        if let Some(prompt) = &self.prompt
            && prompt.chars().count() > MAX_ARBITER_PROMPT_CHARS
        {
            return Err(format!(
                "arbiter.prompt must be at most {MAX_ARBITER_PROMPT_CHARS} characters"
            ));
        }
        Ok(())
    }
}
