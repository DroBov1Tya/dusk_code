//! The three model tiers a thread may hand a subagent: strong, default and weak, each a
//! model of a connected provider the user picks in the settings. A tier left unset is
//! the model of the chat that asks, so nothing changes until the user picks one. The
//! tiers decide nothing else: new chats, titles and the arbiter keep their own models.

use serde::{Deserialize, Serialize};

/// One of the three tiers an asker picks for a subagent. Its schema is in the tools'
/// parameters too, so it derives `JsonSchema` whatever the features.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ModelTier {
    /// Design, review and hard debugging.
    Strong,
    /// Everything else.
    Default,
    /// Mechanical work and read-only scouting.
    Weak,
}

impl ModelTier {
    pub const ALL: [ModelTier; 3] = [ModelTier::Strong, ModelTier::Default, ModelTier::Weak];

    pub fn name(self) -> &'static str {
        match self {
            ModelTier::Strong => "strong",
            ModelTier::Default => "default",
            ModelTier::Weak => "weak",
        }
    }

    pub fn parse(name: &str) -> Option<ModelTier> {
        Self::ALL.into_iter().find(|tier| tier.name() == name)
    }
}

/// The `models` section of the config: each tier as `provider/model`, all left out by
/// default, which keeps every subagent on its chat's model.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelTiers {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strong: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weak: Option<String>,
}

impl ModelTiers {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    pub fn get(&self, tier: ModelTier) -> Option<&str> {
        match tier {
            ModelTier::Strong => self.strong.as_deref(),
            ModelTier::Default => self.default.as_deref(),
            ModelTier::Weak => self.weak.as_deref(),
        }
    }

    pub fn set(&mut self, tier: ModelTier, model: Option<String>) {
        let slot = match tier {
            ModelTier::Strong => &mut self.strong,
            ModelTier::Default => &mut self.default,
            ModelTier::Weak => &mut self.weak,
        };
        *slot = model;
    }

    pub fn validate(&self) -> Result<(), String> {
        for tier in ModelTier::ALL {
            if let Some(model) = self.get(tier)
                && !is_model_name(model)
            {
                return Err(format!(
                    "models.{} must be written as provider/model",
                    tier.name()
                ));
            }
        }
        Ok(())
    }
}

/// Whether `name` is written as `provider/model`, both parts present.
pub fn is_model_name(name: &str) -> bool {
    let named = name
        .split_once('/')
        .is_some_and(|(provider, model)| !provider.trim().is_empty() && !model.trim().is_empty());
    named && !name.chars().any(char::is_control)
}

/// How one tier stands now.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TierState {
    /// What the settings name, as `provider/model`; `None` when unset, which is the
    /// asking chat's model.
    pub chosen: Option<String>,
    /// The chosen model's provider is not connected (removed or switched off), so the
    /// tier is the asking chat's model until it is.
    pub missing: bool,
}

/// Every tier as it stands, for the settings.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TierStates {
    pub strong: TierState,
    pub default: TierState,
    pub weak: TierState,
}

impl TierStates {
    pub fn get(&self, tier: ModelTier) -> &TierState {
        match tier {
            ModelTier::Strong => &self.strong,
            ModelTier::Default => &self.default,
            ModelTier::Weak => &self.weak,
        }
    }
}
