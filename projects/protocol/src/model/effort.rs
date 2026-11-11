//! How hard a model reasons.

use anyhow::{Context, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Reasoning effort. Off is explicit; the provider default omits `reasoning_effort`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Effort {
    Off,
    Minimal,
    Low,
    Medium,
    High,
    Xhigh,
    Max,
    Ultra,
}

impl Effort {
    pub const ALL: [Self; 8] = [
        Self::Off,
        Self::Minimal,
        Self::Low,
        Self::Medium,
        Self::High,
        Self::Xhigh,
        Self::Max,
        Self::Ultra,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Minimal => "minimal",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Xhigh => "xhigh",
            Self::Max => "max",
            Self::Ultra => "ultra",
        }
    }

    /// OpenAI-compatible APIs call explicit Off `none`.
    pub fn wire_name(self) -> &'static str {
        match self {
            Self::Off => "none",
            other => other.name(),
        }
    }

    /// What a subagent thinks at: what its asker chose, medium when it chose nothing.
    /// Off and the provider's default are the user's choice for every thread, so they stay.
    pub fn for_subagent(chat: Option<Self>, asked: Option<Self>) -> Option<Self> {
        match chat {
            None | Some(Self::Off) => chat,
            Some(_) => Some(asked.unwrap_or(Self::Medium)),
        }
    }

    /// For a step that only carries out a plan: low at most, never above this level.
    pub fn light(self) -> Self {
        match self {
            Self::Off | Self::Minimal | Self::Low => self,
            _ => Self::Low,
        }
    }

    /// `default` omits the parameter: the provider decides.
    pub fn parse(text: &str) -> Result<Option<Self>> {
        if text == "default" {
            return Ok(None);
        }
        Self::ALL
            .into_iter()
            .find(|effort| effort.name() == text)
            .map(Some)
            .with_context(|| {
                format!(
                    "unknown effort {text}; off, minimal, low, medium, high, xhigh, max, ultra or default"
                )
            })
    }
}
