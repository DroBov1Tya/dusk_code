//! The models a provider offers.

use anyhow::Result;
use serde::{Deserialize, Serialize};

/// A configured provider as a check found it: its models, or why it did not answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderStatus {
    pub name: String,
    pub models: Result<Vec<Model>, String>,
    /// What is left on the account; none when the provider does not tell it.
    #[serde(default)]
    pub balance: Option<Result<Balance, String>>,
    /// The rate limits its last answer reported; none when it reported nothing.
    #[serde(default)]
    pub limits: Option<RateLimits>,
    /// Off: the provider is kept with its key but not asked, so `models` is empty.
    #[serde(default = "yes")]
    pub enabled: bool,
}

impl ProviderStatus {
    /// A provider that is switched off: not asked, with nothing to offer.
    pub fn disabled(name: String) -> Self {
        Self {
            name,
            models: Ok(Vec::new()),
            balance: None,
            limits: None,
            enabled: false,
        }
    }
}

fn yes() -> bool {
    true
}

/// Money left on a provider account, as the provider wrote it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Balance {
    /// A decimal number; none when the key has no spending limit.
    pub left: Option<String>,
    /// Such as `USD` or `CNY`.
    pub currency: String,
}

impl Balance {
    pub fn text(&self) -> String {
        match &self.left {
            Some(left) => format!("{left} {}", self.currency),
            None => "no limit".into(),
        }
    }
}

/// The rate limits of a provider account, as its last answer reported them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RateLimits {
    pub requests: Option<RateLimit>,
    pub tokens: Option<RateLimit>,
}

/// One limit: how much the account may use in the provider's period, and what is left.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RateLimit {
    pub limit: u64,
    pub remaining: u64,
    /// Seconds until it is full again, counted when the status was made; none when it
    /// is full or the provider did not say.
    pub resets_in: Option<u64>,
}

impl RateLimits {
    /// As every client says it: "4 812 of 5 000 requests, 780k of 800k tokens, resets in 12s".
    pub fn text(&self) -> String {
        let windows = [(&self.requests, "requests"), (&self.tokens, "tokens")];
        let mut parts: Vec<String> = windows
            .iter()
            .filter_map(|(window, what)| Some((window.as_ref()?, what)))
            .map(|(window, what)| {
                let (left, limit) = (count(window.remaining), count(window.limit));
                format!("{left} of {limit} {what}")
            })
            .collect();
        // One time for the line: when the slower of the two is full again.
        let resets_in = windows
            .iter()
            .filter_map(|(window, _)| window.as_ref()?.resets_in)
            .max();
        if let Some(seconds) = resets_in {
            let ms = i64::try_from(seconds).map_or(i64::MAX, |s| s.saturating_mul(1000));
            let wait = crate::thread::duration_words(ms);
            parts.push(format!("resets in {wait}"));
        }
        parts.join(", ")
    }
}

/// Exact with the thousands apart up to 9 999, then `780k`, `1.5M`.
fn count(number: u64) -> String {
    let tenths = |unit: u64| {
        let tenths = number / (unit / 10);
        match tenths % 10 {
            0 => (tenths / 10).to_string(),
            rest => format!("{}.{rest}", tenths / 10),
        }
    };
    match number {
        0..1_000 => number.to_string(),
        1_000..10_000 => format!("{} {:03}", number / 1_000, number % 1_000),
        10_000..1_000_000 => format!("{}k", tenths(1_000)),
        _ => format!("{}M", tenths(1_000_000)),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Model {
    pub id: String,
    /// Maximum input tokens, when the provider's catalogue reports it.
    pub context_window: Option<u64>,
    /// Maximum output tokens of one call, when the catalogue reports it.
    pub max_output: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limits_read_as_one_line() {
        let window = |limit, remaining, resets_in| {
            Some(RateLimit {
                limit,
                remaining,
                resets_in,
            })
        };
        let both = RateLimits {
            requests: window(5_000, 4_812, Some(1)),
            tokens: window(800_000, 780_000, Some(12)),
        };
        assert_eq!(
            both.text(),
            "4 812 of 5 000 requests, 780k of 800k tokens, resets in 12s"
        );
        let tokens = RateLimits {
            requests: None,
            tokens: window(u64::MAX, 1_540_000, None),
        };
        assert_eq!(tokens.text(), "1.5M of 18446744073709.5M tokens");
        // A status saved by an older core has no limits.
        let old: ProviderStatus =
            serde_json::from_str(r#"{"name":"a","models":{"Ok":[]}}"#).unwrap();
        assert_eq!(old.limits, None);
    }
}
