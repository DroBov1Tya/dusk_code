//! Rolling output-rate meter for one agent: tokens per second over a short window,
//! counted as context sizes are. One meter lives per model call.

use std::time::{Duration, Instant};

use crate::providers::ModelEvent;
use crate::state::estimate_tokens;

/// The window the average covers.
pub const WINDOW: Duration = Duration::from_secs(5);
/// How often a metering agent may emit a rate event while it generates.
pub const EMIT_EVERY: Duration = Duration::from_secs(1);

/// Average generated tokens per second over the last `window_ms`, from samples of
/// `(timestamp_ms, cumulative generated tokens)` in order. Each sample is the count as
/// of its own time, so the window is the difference between its bounds' counts.
pub fn rolling_rate(samples: &[(u64, u64)], now_ms: u64, window_ms: u64) -> u64 {
    if samples.is_empty() || window_ms == 0 {
        return 0;
    }
    let cumulative_at = |t: u64| {
        samples
            .iter()
            .rev()
            .find(|(ts, _)| *ts <= t)
            .map_or(0, |(_, cumulative)| *cumulative)
    };
    let start = now_ms.saturating_sub(window_ms);
    let tokens = cumulative_at(now_ms).saturating_sub(cumulative_at(start));
    tokens * 1000 / window_ms
}

/// Samples a stream of model events and answers with a rate when one is due.
#[derive(Debug, Default)]
pub struct RateMeter {
    samples: Vec<(u64, u64)>,
    generated: u64,
    start: Option<Instant>,
    last_emit: Option<Instant>,
}

impl RateMeter {
    /// Notes what the event generated; returns the rate to emit when at least
    /// `EMIT_EVERY` has passed since the last one.
    pub fn observe(&mut self, event: &ModelEvent, now: Instant) -> Option<u64> {
        let text = match event {
            ModelEvent::Reasoning(text) | ModelEvent::Text(text) => text,
            ModelEvent::ToolArguments { fragment, .. } => fragment,
            _ => return None,
        };
        self.add(estimate_tokens(text), now)
    }

    /// The rate as of `now`, at the end of a model call, once it has samples.
    pub fn current(&self, now: Instant) -> Option<u64> {
        let start = self.start?;
        let now_ms = now.duration_since(start).as_millis() as u64;
        Some(rolling_rate(&self.samples, now_ms, WINDOW.as_millis() as u64))
    }

    fn add(&mut self, tokens: u64, now: Instant) -> Option<u64> {
        if tokens == 0 {
            return None;
        }
        let start = *self.start.get_or_insert(now);
        self.generated += tokens;
        let now_ms = now.duration_since(start).as_millis() as u64;
        self.samples.push((now_ms, self.generated));
        let rate = rolling_rate(&self.samples, now_ms, WINDOW.as_millis() as u64);
        match self.last_emit {
            // The first sample of a call reports at once.
            None => {
                self.last_emit = Some(now);
                Some(rate)
            }
            Some(previous) if now.duration_since(previous) >= EMIT_EVERY => {
                self.last_emit = Some(now);
                Some(rate)
            }
            Some(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::rolling_rate;

    #[test]
    fn rolling_window_average_of_a_synthetic_stream() {
        // (timestamp_ms, cumulative generated tokens).
        let mut samples = vec![
            (0, 0),
            (1_000, 30),
            (2_000, 70),
            (3_000, 120),
            (4_000, 180),
            (5_000, 260),
        ];
        // The whole span falls in the window: 260 tokens over 5 s.
        assert_eq!(rolling_rate(&samples, 5_000, 5_000), 52);
        samples.extend([(6_000, 320), (7_000, 400), (8_000, 500)]);
        // The window slides: the 380 tokens since its start over 5 s.
        assert_eq!(rolling_rate(&samples, 8_000, 5_000), 76);
        // A shorter window counts only the samples inside it.
        assert_eq!(rolling_rate(&samples, 8_000, 2_000), 90);
        // No samples is 0, and so is a window in which nothing new was generated.
        assert_eq!(rolling_rate(&[], 8_000, 5_000), 0);
        assert_eq!(rolling_rate(&[(1_000, 20)], 10_000, 5_000), 0);
    }
}
