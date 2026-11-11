//! What a call used and cost.

use serde::{Deserialize, Serialize};

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
