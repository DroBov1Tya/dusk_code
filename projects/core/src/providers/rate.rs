//! A request rate the user set for a provider, kept on this side, so a goal loop or a
//! crowd of subagents cannot get the key throttled or banned.

use std::time::Duration;

use parking_lot::Mutex;
use tokio::time::Instant;

pub struct TokenBucket {
    per_second: f64,
    capacity: f64,
    /// Tokens left, negative while callers wait their turn, and when it was counted.
    state: Mutex<(f64, Instant)>,
}

impl TokenBucket {
    /// A full bucket: a burst of `per_minute` requests, then one every `60 / per_minute`
    /// seconds.
    pub fn new(per_minute: u32) -> Self {
        let capacity = f64::from(per_minute.max(1));
        Self {
            per_second: capacity / 60.0,
            capacity,
            state: Mutex::new((capacity, Instant::now())),
        }
    }

    /// Waits until a request may go. The token is taken at once, so callers that wait
    /// are served in the order they came.
    pub async fn take(&self) {
        let wait = {
            let mut state = self.state.lock();
            let now = Instant::now();
            let refilled = state.0 + (now - state.1).as_secs_f64() * self.per_second;
            state.0 = refilled.min(self.capacity) - 1.0;
            state.1 = now;
            (state.0 < 0.0).then(|| Duration::from_secs_f64(-state.0 / self.per_second))
        };
        if let Some(wait) = wait {
            tracing::info!("provider rate limit: waiting {} ms", wait.as_millis());
            tokio::time::sleep(wait).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 6000 a minute is one every 10 ms after the burst.
    #[tokio::test]
    async fn a_burst_passes_then_requests_are_spaced() {
        let bucket = TokenBucket::new(6_000);
        let start = Instant::now();
        for _ in 0..6_000 {
            bucket.take().await;
        }
        let burst = start.elapsed();
        assert!(burst < Duration::from_millis(15), "the burst is free");
        for _ in 0..5 {
            bucket.take().await;
        }
        assert!(start.elapsed() - burst >= Duration::from_millis(45));
    }
}
