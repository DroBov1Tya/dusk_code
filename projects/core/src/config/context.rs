//! A model's input budget.

use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

/// The window taken for a model whose provider reports none and that has no size set
/// under `/settings` > models.
pub const UNKNOWN_WINDOW: u64 = 128_000;

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
