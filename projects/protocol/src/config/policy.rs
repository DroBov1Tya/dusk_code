//! Approval and context policies.

use anyhow::{Result, bail, ensure};
use serde::{Deserialize, Serialize};

/// The window taken for a model whose provider reports none and that has no size set
/// under `/settings` > models.
pub const UNKNOWN_WINDOW: u64 = 128_000;

/// Ask is the default. Sandbox runs commands without asking, confined by the system to
/// the working folders; paths outside them and the screen are still asked. Full access
/// answers every request with yes. Arbiter hands each request to an agent that decides
/// in the user's place. The config folder stays closed either way.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalPolicy {
    #[default]
    Ask,
    Sandbox,
    Arbiter,
    FullAccess,
}

impl ApprovalPolicy {
    pub fn name(self) -> &'static str {
        match self {
            Self::Ask => "ask",
            Self::Sandbox => "sandbox",
            Self::Arbiter => "arbiter",
            Self::FullAccess => "full access",
        }
    }

    pub fn is_ask(&self) -> bool {
        *self == Self::Ask
    }

    /// The name the config file and state records keep, for a thread's own mode.
    pub fn stored_name(self) -> &'static str {
        match self {
            Self::Ask => "ask",
            Self::Sandbox => "sandbox",
            Self::Arbiter => "arbiter",
            Self::FullAccess => "full_access",
        }
    }

    /// The policy stored under `name`; an error for an unknown name.
    pub fn parse_stored(name: &str) -> Result<Self> {
        match name {
            "ask" => Ok(Self::Ask),
            "sandbox" => Ok(Self::Sandbox),
            "arbiter" => Ok(Self::Arbiter),
            "full_access" => Ok(Self::FullAccess),
            _ => bail!("unknown approval policy {name:?}"),
        }
    }
}

/// A model's input budget. Auto is the model's own window as its provider reports it,
/// and [`UNKNOWN_WINDOW`] for a model that reports none. Fixed and Manual are set by
/// hand; a known window stays their ceiling.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextPolicy {
    #[default]
    Auto,
    Fixed(ContextPreset),
    Manual(u64),
}

/// Decimal token counts, matching model catalogue context windows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ContextPreset {
    #[serde(rename = "128k")]
    K128,
    #[serde(rename = "256k")]
    K256,
    #[serde(rename = "512k")]
    K512,
    #[serde(rename = "1m")]
    M1,
}

impl ContextPreset {
    pub const ALL: [Self; 4] = [Self::K128, Self::K256, Self::K512, Self::M1];

    pub fn tokens(self) -> u64 {
        match self {
            Self::K128 => 128_000,
            Self::K256 => 256_000,
            Self::K512 => 512_000,
            Self::M1 => 1_000_000,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::K128 => "128k",
            Self::K256 => "256k",
            Self::K512 => "512k",
            Self::M1 => "1M",
        }
    }
}

impl ContextPolicy {
    pub fn validate(self) -> Result<()> {
        ensure!(
            !matches!(self, Self::Manual(0)),
            "manual context must be above zero"
        );
        Ok(())
    }

    /// The single resolver used by request guards, context reports and compaction.
    pub fn input_limit(self, model_window: Option<u64>) -> u64 {
        let selected = match self {
            Self::Auto => return model_window.unwrap_or(UNKNOWN_WINDOW),
            Self::Fixed(preset) => preset.tokens(),
            Self::Manual(tokens) => tokens,
        };
        model_window.map_or(selected, |window| window.min(selected))
    }
}
