//! The rate limits of an account, as its provider last reported them in response headers.
//! Kept in memory only and never asked for: they are read off the answers that come anyway.

use std::{
    collections::HashMap,
    sync::LazyLock,
    time::{Duration, Instant},
};

use parking_lot::Mutex;
use reqwest::header::HeaderMap;
use sha2::{Digest, Sha256};

use crate::config::ProviderConfig;

pub use dusk_protocol::model::{PlanWindow, RateLimit, RateLimits};

/// No provider counts further than this; a larger number is not a limit.
const MAX_COUNT: u64 = 1_000_000_000_000;
/// The longest period a limit is counted over is a day.
const MAX_RESET: Duration = Duration::from_secs(24 * 60 * 60);
/// The longest usage window a plan has is about a month.
const MAX_PLAN_WINDOW: Duration = Duration::from_secs(31 * 24 * 60 * 60);
/// Longer than any number, duration or timestamp a provider writes.
const MAX_VALUE_BYTES: usize = 40;

/// Providers are built anew from the config all the time, so what one saw is kept by
/// account, for the next one of the same address and key.
static SEEN: LazyLock<Mutex<HashMap<[u8; 32], Seen>>> = LazyLock::new(Mutex::default);

/// Which headers a protocol writes its limits in.
#[derive(Clone, Copy)]
pub enum Headers {
    /// `x-ratelimit-{limit,remaining,reset}-{requests,tokens}`, the reset as `6m0s`.
    OpenAi,
    /// `anthropic-ratelimit-{requests,tokens}-{limit,remaining,reset}`, the reset as an
    /// RFC 3339 time.
    Anthropic,
    /// `x-codex-{primary,secondary}-{used-percent,window-minutes,reset-after-seconds,
    /// reset-at}`: the ChatGPT plan's usage windows by the share used.
    ChatGpt,
}

/// An address and its key, as a digest: the key itself is not kept here.
pub struct Account([u8; 32]);

#[derive(Default)]
struct Seen {
    requests: Option<Window>,
    tokens: Option<Window>,
    plan: Vec<Share>,
}

/// A plan's usage window as last seen.
struct Share {
    minutes: Option<u64>,
    used_percent: u8,
    reset: Option<Instant>,
}

struct Window {
    limit: u64,
    remaining: u64,
    /// When the provider said the limit is full again.
    reset: Option<Instant>,
}

impl Account {
    pub fn of(settings: &ProviderConfig) -> Self {
        let mut hash = Sha256::new();
        hash.update(settings.base_url.as_bytes());
        hash.update([0]);
        hash.update(settings.api_key.0.as_bytes());
        Self(hash.finalize().into())
    }

    /// Remembers the limits a response carries; one without them changes nothing.
    pub fn note(&self, headers: Headers, response: &HeaderMap) {
        let now = Instant::now();
        if matches!(headers, Headers::ChatGpt) {
            let plan: Vec<Share> = ["primary", "secondary"]
                .into_iter()
                .filter_map(|which| plan_share(response, which, now))
                .collect();
            if !plan.is_empty() {
                SEEN.lock().entry(self.0).or_default().plan = plan;
            }
            return;
        }
        let (requests, tokens) = match headers {
            Headers::OpenAi => (
                openai_window(response, "requests", now),
                openai_window(response, "tokens", now),
            ),
            Headers::Anthropic => (
                anthropic_window(response, "requests", now),
                // All tokens, or the input ones when only those are counted.
                anthropic_window(response, "tokens", now)
                    .or_else(|| anthropic_window(response, "input-tokens", now)),
            ),
            Headers::ChatGpt => return,
        };
        if requests.is_none() && tokens.is_none() {
            return;
        }
        let mut seen = SEEN.lock();
        let account = seen.entry(self.0).or_default();
        if requests.is_some() {
            account.requests = requests;
        }
        if tokens.is_some() {
            account.tokens = tokens;
        }
    }

    /// What was last seen, as it stands now; none when the provider reported nothing.
    pub fn limits(&self) -> Option<RateLimits> {
        let now = Instant::now();
        let seen = SEEN.lock();
        let account = seen.get(&self.0)?;
        Some(RateLimits {
            requests: account.requests.as_ref().map(|window| window.at(now)),
            tokens: account.tokens.as_ref().map(|window| window.at(now)),
            plan: account.plan.iter().map(|share| share.at(now)).collect(),
        })
    }
}

impl Share {
    fn at(&self, now: Instant) -> PlanWindow {
        let wait = self
            .reset
            .map(|reset| reset.saturating_duration_since(now))
            .filter(|wait| !wait.is_zero());
        PlanWindow {
            minutes: self.minutes,
            // Past its reset the window starts empty again.
            used_percent: match (self.reset, wait) {
                (Some(_), None) => 0,
                _ => self.used_percent,
            },
            resets_in: wait.map(|wait| wait.as_secs().max(1)),
        }
    }
}

impl Window {
    fn at(&self, now: Instant) -> RateLimit {
        let wait = self
            .reset
            .map(|reset| reset.saturating_duration_since(now))
            .filter(|wait| !wait.is_zero());
        RateLimit {
            limit: self.limit,
            // Past its reset the limit is full again, whatever was left before.
            remaining: match (self.reset, wait) {
                (Some(_), None) => self.limit,
                _ => self.remaining,
            },
            resets_in: wait.map(|wait| wait.as_secs().max(1)),
        }
    }
}

fn openai_window(headers: &HeaderMap, what: &str, now: Instant) -> Option<Window> {
    let value = |part: &str| value(headers, &format!("x-ratelimit-{part}-{what}"));
    window(
        count(value("limit")?)?,
        count(value("remaining")?)?,
        value("reset").and_then(span).map(|wait| now + wait),
    )
}

fn anthropic_window(headers: &HeaderMap, what: &str, now: Instant) -> Option<Window> {
    let value = |part: &str| value(headers, &format!("anthropic-ratelimit-{what}-{part}"));
    window(
        count(value("limit")?)?,
        count(value("remaining")?)?,
        value("reset").and_then(until).map(|wait| now + wait),
    )
}

/// One of the ChatGPT plan's windows; the reset comes as seconds to wait or, from newer
/// servers, as a Unix time.
fn plan_share(headers: &HeaderMap, which: &str, now: Instant) -> Option<Share> {
    let value = |part: &str| value(headers, &format!("x-codex-{which}-{part}"));
    let used: f64 = value("used-percent")?.parse().ok()?;
    if !(0.0..=1000.0).contains(&used) {
        return None;
    }
    let minutes = value("window-minutes")
        .and_then(count)
        .filter(|minutes| (1..=MAX_PLAN_WINDOW.as_secs() / 60).contains(minutes));
    let wait = value("reset-after-seconds")
        .and_then(count)
        .map(Duration::from_secs)
        .or_else(|| value("reset-at").and_then(count).and_then(until_unix))
        .filter(|wait| *wait <= MAX_PLAN_WINDOW);
    Some(Share {
        minutes,
        used_percent: used.round().min(100.) as u8,
        reset: wait.map(|wait| now + wait),
    })
}

/// The wait until a Unix time in seconds; zero when it has passed.
fn until_unix(seconds: u64) -> Option<Duration> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?;
    Some(Duration::from_secs(seconds).saturating_sub(now))
}

fn window(limit: u64, remaining: u64, reset: Option<Instant>) -> Option<Window> {
    (limit > 0).then(|| Window {
        limit,
        remaining: remaining.min(limit),
        reset,
    })
}

/// A header as short plain text; anything else is not a limit.
fn value<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    let text = headers.get(name)?.to_str().ok()?.trim();
    (!text.is_empty() && text.len() <= MAX_VALUE_BYTES).then_some(text)
}

fn count(text: &str) -> Option<u64> {
    let digits = text.bytes().all(|byte| byte.is_ascii_digit());
    let number = text.parse::<u64>().ok().filter(|_| digits)?;
    (number <= MAX_COUNT).then_some(number)
}

/// A wait as OpenAI writes it: `20ms`, `1s`, `7.66s`, `6m0s`, `1h2m3s`.
fn span(text: &str) -> Option<Duration> {
    let mut seconds = 0.0;
    let mut rest = text;
    while !rest.is_empty() {
        let end = rest.find(|c: char| !c.is_ascii_digit() && c != '.')?;
        let number: f64 = rest[..end].parse().ok()?;
        let (unit, after) = [("ms", 0.001), ("h", 3600.0), ("m", 60.0), ("s", 1.0)]
            .iter()
            .find_map(|(suffix, unit)| Some((unit, rest[end..].strip_prefix(suffix)?)))?;
        seconds += number * unit;
        rest = after;
    }
    (!text.is_empty() && seconds.is_finite() && seconds <= MAX_RESET.as_secs_f64())
        .then(|| Duration::from_secs_f64(seconds))
}

/// The wait until an RFC 3339 time, as Anthropic writes a reset; nothing when it has
/// passed.
fn until(text: &str) -> Option<Duration> {
    let reset = chrono::DateTime::parse_from_rfc3339(text).ok()?;
    let wait = reset.signed_duration_since(chrono::Utc::now());
    let wait = wait.to_std().unwrap_or(Duration::ZERO);
    (wait <= MAX_RESET).then_some(wait)
}

#[cfg(test)]
mod tests {
    use reqwest::header::{HeaderName, HeaderValue};

    use super::*;

    fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
        pairs
            .iter()
            .map(|(name, value)| {
                (
                    HeaderName::from_bytes(name.as_bytes()).unwrap(),
                    HeaderValue::from_bytes(value.as_bytes()).unwrap(),
                )
            })
            .collect()
    }

    fn account(name: &str) -> Account {
        let settings = serde_json::json!({"base_url": format!("https://{name}.example/v1")});
        Account::of(&serde_json::from_value(settings).unwrap())
    }

    #[test]
    fn reads_the_openai_headers() {
        let account = account("openai-headers");
        account.note(
            Headers::OpenAi,
            &headers(&[
                ("x-ratelimit-limit-requests", "5000"),
                ("x-ratelimit-remaining-requests", "4812"),
                ("x-ratelimit-reset-requests", "12s"),
                ("x-ratelimit-limit-tokens", "800000"),
                ("x-ratelimit-remaining-tokens", "780000"),
                ("x-ratelimit-reset-tokens", "6m0s"),
            ]),
        );
        let limits = account.limits().unwrap();
        let requests = limits.requests.unwrap();
        assert_eq!((requests.limit, requests.remaining), (5_000, 4_812));
        assert!(matches!(requests.resets_in, Some(11..=12)));
        let tokens = limits.tokens.unwrap();
        assert_eq!((tokens.limit, tokens.remaining), (800_000, 780_000));
        assert!(matches!(tokens.resets_in, Some(359..=360)));

        // An answer without the headers keeps what was seen; one with a part replaces it.
        account.note(Headers::OpenAi, &HeaderMap::new());
        account.note(
            Headers::OpenAi,
            &headers(&[
                ("x-ratelimit-limit-requests", "5000"),
                ("x-ratelimit-remaining-requests", "9999"),
            ]),
        );
        let limits = account.limits().unwrap();
        let requests = limits.requests.unwrap();
        assert_eq!((requests.remaining, requests.resets_in), (5_000, None));
        assert_eq!(limits.tokens.unwrap().remaining, 780_000);
    }

    #[test]
    fn reads_the_anthropic_headers() {
        let account = account("anthropic-headers");
        let soon = (chrono::Utc::now() + chrono::Duration::seconds(30)).to_rfc3339();
        account.note(
            Headers::Anthropic,
            &headers(&[
                ("anthropic-ratelimit-requests-limit", "4000"),
                ("anthropic-ratelimit-requests-remaining", "3999"),
                ("anthropic-ratelimit-requests-reset", &soon),
                ("anthropic-ratelimit-input-tokens-limit", "2000000"),
                ("anthropic-ratelimit-input-tokens-remaining", "1990000"),
                // Already full again: nothing to wait for, and nothing is used up.
                (
                    "anthropic-ratelimit-input-tokens-reset",
                    "2020-01-01T00:00:00Z",
                ),
            ]),
        );
        let limits = account.limits().unwrap();
        let requests = limits.requests.unwrap();
        assert_eq!((requests.limit, requests.remaining), (4_000, 3_999));
        assert!(matches!(requests.resets_in, Some(28..=30)));
        let tokens = limits.tokens.unwrap();
        assert_eq!(
            (tokens.limit, tokens.remaining, tokens.resets_in),
            (2_000_000, 2_000_000, None)
        );
    }

    #[test]
    fn reads_the_chatgpt_plan_headers() {
        let account = account("chatgpt-headers");
        let week = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
            + 3 * 24 * 3600;
        account.note(
            Headers::ChatGpt,
            &headers(&[
                ("x-codex-primary-used-percent", "5.0"),
                ("x-codex-primary-window-minutes", "300"),
                ("x-codex-primary-reset-after-seconds", "7800"),
                ("x-codex-secondary-used-percent", "100"),
                ("x-codex-secondary-window-minutes", "10080"),
                ("x-codex-secondary-reset-at", &week.to_string()),
            ]),
        );
        let limits = account.limits().unwrap();
        assert_eq!((&limits.requests, &limits.tokens), (&None, &None));
        let [short, long] = limits.plan.as_slice() else {
            panic!("two windows: {:?}", limits.plan);
        };
        assert_eq!((short.minutes, short.used_percent), (Some(300), 5));
        assert!(matches!(short.resets_in, Some(7_799..=7_800)));
        assert_eq!((long.minutes, long.used_percent), (Some(10_080), 100));
        assert!(matches!(long.resets_in, Some(259_190..=259_200)));
        assert_eq!(limits.used(), Some(1.));

        // Headers that cannot be read leave what was seen.
        for bad in ["", "-1", "nan", "inf", "1e9", "abc"] {
            account.note(
                Headers::ChatGpt,
                &headers(&[("x-codex-primary-used-percent", bad)]),
            );
        }
        assert_eq!(account.limits().unwrap().plan.len(), 2);
        // The ChatGPT headers say nothing about requests and tokens, nor the reverse.
        account.note(
            Headers::OpenAi,
            &headers(&[("x-codex-primary-used-percent", "50")]),
        );
        assert_eq!(account.limits().unwrap().plan[0].used_percent, 5);
    }

    #[test]
    fn refuses_malformed_and_oversized_values() {
        for bad in [
            "",
            "-1",
            "+5",
            "1e3",
            "5 000",
            "1.5",
            "0x10",
            "٣",
            "1000000000001",
        ] {
            assert_eq!(count(bad), None, "{bad}");
        }
        assert_eq!(count("1000000000000"), Some(MAX_COUNT));

        for bad in [
            "", "12", "s", "1x", "-1s", "1s ", "1.2.3s", ".s", "1e9s", "25h", "nans", "infs",
        ] {
            assert_eq!(span(bad), None, "{bad}");
        }
        assert_eq!(span("20ms"), Some(Duration::from_millis(20)));
        assert_eq!(span("1h2m3s"), Some(Duration::from_secs(3723)));
        assert_eq!(span("24h"), Some(MAX_RESET));

        for bad in ["", "tomorrow", "1700000000", "2999-01-01T00:00:00Z"] {
            assert_eq!(until(bad), None, "{bad}");
        }

        let account = account("malformed-headers");
        let long = "9".repeat(MAX_VALUE_BYTES + 1);
        for (limit, remaining) in [
            ("0", "0"),
            ("abc", "1"),
            ("10", ""),
            (long.as_str(), "1"),
            ("10", "\u{e9}"),
        ] {
            account.note(
                Headers::OpenAi,
                &headers(&[
                    ("x-ratelimit-limit-requests", limit),
                    ("x-ratelimit-remaining-requests", remaining),
                ]),
            );
            assert!(account.limits().is_none(), "{limit} {remaining}");
        }
        // A reset that cannot be read leaves the counts without a time.
        account.note(
            Headers::OpenAi,
            &headers(&[
                ("x-ratelimit-limit-tokens", "100"),
                ("x-ratelimit-remaining-tokens", "40"),
                ("x-ratelimit-reset-tokens", "soon"),
            ]),
        );
        let tokens = account.limits().unwrap().tokens.unwrap();
        assert_eq!((tokens.remaining, tokens.resets_in), (40, None));
    }
}
