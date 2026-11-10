//! What a model streams, the tool calls it makes, and how hard it reasons.

use anyhow::{Context, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// One increment of a streamed model response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ModelEvent {
    /// Model thinking: shown to the user and never sent back as context.
    Reasoning(String),
    Text(String),
    /// A new tool call; its arguments follow as `ToolArguments` with the same index.
    ToolCall {
        index: u32,
        id: String,
        name: String,
    },
    /// A piece of the JSON arguments of the call with this index.
    ToolArguments {
        index: u32,
        fragment: String,
    },
    Usage(Usage),
    Finished(FinishReason),
}

/// Token usage of the whole request, as the provider reports it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
    /// Everything the request sent, the cached part included.
    pub prompt_tokens: u64,
    /// The part of the prompt the provider served from its cache, usually at a discount.
    pub cached_prompt_tokens: u64,
    /// Everything the model wrote, its reasoning included.
    pub completion_tokens: u64,
    pub reasoning_tokens: u64,
    /// The part of the prompt written to the cache, which some providers bill above
    /// plain input.
    #[serde(default)]
    pub cache_write_tokens: u64,
    /// What the call cost, when the provider said so or the user's price list has the
    /// model; never guessed.
    #[serde(default)]
    pub cost: Option<Cost>,
}

/// A price in millionths of a US dollar and where it came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cost {
    pub micro_usd: u64,
    pub source: CostSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CostSource {
    /// The provider reported it with the response.
    Provider,
    /// Counted from the price list in the provider's config.
    Prices,
}

impl Cost {
    /// Dollars with the precision a small amount needs: `$0.0042`, `$1.27`.
    pub fn dollars(micro_usd: u64) -> String {
        let dollars = micro_usd as f64 / 1_000_000.0;
        if micro_usd >= 10_000 {
            format!("${dollars:.2}")
        } else {
            format!("${dollars:.4}")
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FinishReason {
    Stop,
    ToolCalls,
    /// Cut by the output token limit.
    Length,
    ContentFilter,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCall {
    /// Provider-assigned; a tool result refers back to it.
    pub id: String,
    pub name: String,
    pub arguments: Value,
}

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
