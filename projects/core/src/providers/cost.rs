//! What a model call cost: as the provider reported it, else counted from the price list
//! in the provider's config. Never guessed: without either it stays unknown.

use anyhow::{Result, ensure};

use crate::config::Price;

pub use dusk_protocol::model::{Cost, CostSource};

use super::Usage;

/// Above any real price per million tokens; a larger one is a typo.
const MAX_PRICE: f64 = 10_000.0;

/// Dollars as a provider reports them, such as OpenRouter's `usage.cost`.
pub fn reported_usd(usd: f64) -> Option<Cost> {
    micro_usd(usd * 1_000_000.0).map(provider)
}

/// xAI's `cost_in_usd_ticks`: ten-billionths of a dollar.
pub fn reported_ticks(ticks: u64) -> Cost {
    provider(ticks.saturating_add(5_000) / 10_000)
}

/// The cost of `usage` at `price`, each kind of token at its own rate.
pub fn priced(price: &Price, usage: &Usage) -> Option<Cost> {
    let cached = usage.cached_prompt_tokens;
    let written = usage.cache_write_tokens;
    let plain = usage
        .prompt_tokens
        .saturating_sub(cached)
        .saturating_sub(written);
    // Tokens times dollars per million tokens is millionths of a dollar.
    let micro = plain as f64 * price.input
        + cached as f64 * price.cached_input.unwrap_or(price.input)
        + written as f64 * price.cache_write.unwrap_or(price.input)
        + usage.completion_tokens as f64 * price.output;
    micro_usd(micro).map(|micro_usd| Cost {
        micro_usd,
        source: CostSource::Prices,
    })
}

/// Every rate is a number from zero to `MAX_PRICE`.
pub fn check(model: &str, price: &Price) -> Result<()> {
    let rates = [
        Some(price.input),
        price.cached_input,
        price.cache_write,
        Some(price.output),
    ];
    for rate in rates.into_iter().flatten() {
        ensure!(
            rate.is_finite() && (0.0..=MAX_PRICE).contains(&rate),
            "the price of {model} must be dollars per million tokens, from 0 to {MAX_PRICE}"
        );
    }
    Ok(())
}

fn provider(micro_usd: u64) -> Cost {
    Cost {
        micro_usd,
        source: CostSource::Provider,
    }
}

/// A count a provider or a sum gave; negative or not a number is no cost at all.
fn micro_usd(micro: f64) -> Option<u64> {
    (micro.is_finite() && micro >= 0.0 && micro < u64::MAX as f64).then(|| micro.round() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_kind_of_token_has_its_rate() {
        let price = Price {
            input: 3.0,
            cached_input: Some(0.3),
            cache_write: Some(3.75),
            output: 15.0,
        };
        let usage = Usage {
            prompt_tokens: 1_000_000 + 2_000_000 + 100_000,
            cached_prompt_tokens: 2_000_000,
            cache_write_tokens: 100_000,
            completion_tokens: 10_000,
            ..Usage::default()
        };
        // 3.00 + 0.60 + 0.375 + 0.15 dollars.
        assert_eq!(priced(&price, &usage).unwrap().micro_usd, 4_125_000);
    }

    #[test]
    fn reported_costs_convert_and_bad_ones_are_unknown() {
        assert_eq!(reported_usd(0.0042).unwrap().micro_usd, 4_200);
        assert_eq!(reported_ticks(37_756_000).micro_usd, 3_776);
        assert!(reported_usd(-1.0).is_none());
        assert!(reported_usd(f64::NAN).is_none());
        let typo = Price {
            input: f64::INFINITY,
            cached_input: None,
            cache_write: None,
            output: 1.0,
        };
        assert!(check("model", &typo).is_err());
    }
}
