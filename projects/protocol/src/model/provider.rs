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
    /// A subscription's usage windows, such as ChatGPT's five hours and week, by the
    /// share used; empty for a provider that counts requests and tokens.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub plan: Vec<PlanWindow>,
}

/// One usage window of a subscription: how long it is and how much of it is used.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanWindow {
    /// How long the window is, in minutes; none when the provider did not say.
    pub minutes: Option<u64>,
    /// From 0 to 100.
    pub used_percent: u8,
    /// Seconds until it is empty again, counted when the status was made.
    pub resets_in: Option<u64>,
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
    /// The largest share used of any limit, from 0 to 1; none when no limit has a size.
    pub fn used(&self) -> Option<f32> {
        let counted = [&self.requests, &self.tokens]
            .into_iter()
            .flatten()
            .filter(|window| window.limit > 0)
            .map(|window| 1. - window.remaining as f32 / window.limit as f32);
        let plan = self
            .plan
            .iter()
            .map(|window| f32::from(window.used_percent) / 100.);
        counted
            .chain(plan)
            .map(|share| share.clamp(0., 1.))
            .reduce(f32::max)
    }

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
        for window in &self.plan {
            let size = window.minutes.map_or_else(
                || "plan".to_owned(),
                |minutes| format!("{} plan", span(minutes)),
            );
            let mut part = format!("{}% of the {size} limit used", window.used_percent);
            if let Some(seconds) = window.resets_in {
                let ms = i64::try_from(seconds).map_or(i64::MAX, |s| s.saturating_mul(1000));
                part.push_str(&format!(
                    " (resets in {})",
                    crate::thread::duration_words(ms)
                ));
            }
            parts.push(part);
        }
        parts.join(", ")
    }
}

/// A window's length as a plan names it: `5h`, `7d`, `90m`.
fn span(minutes: u64) -> String {
    match minutes {
        0..60 => format!("{minutes}m"),
        _ if minutes.is_multiple_of(1440) => format!("{}d", minutes / 1440),
        _ if minutes.is_multiple_of(60) => format!("{}h", minutes / 60),
        _ => format!("{}h {}m", minutes / 60, minutes % 60),
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
            plan: Vec::new(),
        };
        assert_eq!(
            both.text(),
            "4 812 of 5 000 requests, 780k of 800k tokens, resets in 12s"
        );
        let tokens = RateLimits {
            requests: None,
            tokens: window(u64::MAX, 1_540_000, None),
            plan: Vec::new(),
        };
        assert_eq!(tokens.text(), "1.5M of 18446744073709.5M tokens");
        let plan = RateLimits {
            requests: None,
            tokens: None,
            plan: vec![
                PlanWindow {
                    minutes: Some(300),
                    used_percent: 5,
                    resets_in: Some(7_800),
                },
                PlanWindow {
                    minutes: Some(10_080),
                    used_percent: 40,
                    resets_in: None,
                },
            ],
        };
        assert_eq!(
            plan.text(),
            "5% of the 5h plan limit used (resets in 2h 10m), 40% of the 7d plan limit used"
        );
        assert_eq!(plan.used(), Some(0.4));
        // A status saved by an older core has no limits.
        let old: ProviderStatus =
            serde_json::from_str(r#"{"name":"a","models":{"Ok":[]}}"#).unwrap();
        assert_eq!(old.limits, None);
    }
}
