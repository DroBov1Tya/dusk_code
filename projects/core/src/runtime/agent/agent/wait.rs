//! A turn that waits for its provider. A model call failed in a way that passes by itself,
//! such as a rate limit, a spent quota or a host out of reach, and its attempts are spent:
//! the turn sleeps and takes the step again instead of failing. The wait is stored with the
//! turn, so a restart sleeps out the rest of it; the user's stop ends it, and the user's
//! message cuts it short, since the user may have fixed the key or the network.

use std::time::Duration;

use anyhow::Result;
use dusk_protocol::model::{ProviderWaitKind, RateLimits};
use dusk_protocol::system::now_ms;
use dusk_protocol::thread::TurnWait;
use tracing::info;

use super::Agent;
use super::retry::UNIT;
use crate::providers::{Account, ProviderError, ProviderErrorKind};
use crate::runtime::ThreadEvent;
use crate::runtime::agent::{Inbox, Stop};
use crate::state::Turn;

/// Waits in a row end this long after the first began; the turn then fails as it did
/// before turns waited.
const MAX_WAITING: Duration = UNIT.saturating_mul(24 * 3600);
/// No wait reaches further ahead, whatever the provider says.
const MAX_AHEAD: Duration = UNIT.saturating_mul(6 * 3600);
/// With no word of when the provider is back, a wait is half the time waited so far,
/// within these.
const MIN_PAUSE: Duration = UNIT.saturating_mul(60);
const MAX_PAUSE: Duration = UNIT.saturating_mul(15 * 60);
/// A spent quota is rarely back within minutes.
const QUOTA_PAUSE: Duration = UNIT.saturating_mul(30 * 60);

/// How a wait for the provider ended.
pub(super) enum Waited {
    /// It is over: the step is taken again. `since` is when the waits in a row began.
    Again { since: i64 },
    /// The user stopped the turn.
    Stopped(Stop),
    /// The waits in a row have lasted long enough: the turn fails.
    Spent,
}

/// Whether a turn waits out a failure of `kind` instead of failing.
pub(super) fn waits_out(kind: ProviderErrorKind) -> bool {
    ProviderWaitKind::try_from(kind).is_ok()
}

impl Agent {
    /// Stores a wait for the provider that failed with `error`, sleeps it out and says
    /// how it ended. `since` is when the waits in a row before this one began.
    pub(super) async fn wait_for_provider(
        &self,
        turn: &Turn,
        error: &ProviderError,
        since: Option<i64>,
        inbox: &mut Inbox,
    ) -> Result<Waited> {
        let now = now_ms();
        let since = since.unwrap_or(now);
        let until = since.saturating_add(millis(MAX_WAITING));
        if now >= until {
            info!(
                "thread {} turn {} waited long enough for the provider",
                self.thread_id, turn.turn_id
            );
            return Ok(Waited::Spent);
        }
        let wake_at = now
            .saturating_add(millis(self.pause_for(error, now - since)))
            .min(until);
        let waiting = self
            .state
            .wait_for_provider(&turn.turn_id, error.kind, wake_at)
            .await?;
        let since = match waiting.waiting {
            Some(TurnWait::Provider { since, .. }) => since,
            _ => since,
        };
        info!(
            "thread {} turn {} waits {} ms for the provider: {:?}",
            self.thread_id,
            turn.turn_id,
            wake_at - now,
            error.kind
        );
        Ok(match self.sleep_out(&waiting, wake_at, inbox).await? {
            Ok(()) => Waited::Again { since },
            Err(stop) => Waited::Stopped(stop),
        })
    }

    /// Sleeps until `wake_at` (Unix ms), told to clients as the turn's wait: only the
    /// user's stop ends it before, and only a message the user sent meanwhile wakes it
    /// early. Clients hear the turn runs again once it is over.
    pub(super) async fn sleep_out(
        &self,
        turn: &Turn,
        wake_at: i64,
        inbox: &mut Inbox,
    ) -> Result<Result<(), Stop>> {
        self.emit(ThreadEvent::TurnWaiting(turn.clone()));
        // Messages sent before the wait are not the user's answer to it.
        let seen = self.newest_user_message().await?;
        loop {
            let left = wake_at.saturating_sub(now_ms());
            if left <= 0 {
                break;
            }
            let left = Duration::from_millis(u64::try_from(left).unwrap_or(0));
            if let Err(stop) = inbox.pause(left).await {
                return Ok(Err(stop));
            }
            if self.newest_user_message().await? > seen {
                info!("thread {} wakes early for the user", self.thread_id);
                break;
            }
        }
        let running = Turn {
            waiting: None,
            ..turn.clone()
        };
        self.emit(ThreadEvent::TurnRunning(running));
        Ok(Ok(()))
    }

    /// The newest message the user sent that the thread has not taken in yet.
    async fn newest_user_message(&self) -> Result<Option<i64>> {
        let pending = self.state.pending_messages(&self.thread_id).await?;
        Ok(pending
            .iter()
            .filter(|message| message.from_thread_id.is_none() && message.from_job_id.is_none())
            .map(|message| message.pending_id)
            .max())
    }

    /// How long to wait after `error`, `waited` ms into the waits in a row: as long as the
    /// provider asked, or until its limits are full again, or half the time waited so far.
    fn pause_for(&self, error: &ProviderError, waited: i64) -> Duration {
        let limited = matches!(
            error.kind,
            ProviderErrorKind::RateLimited | ProviderErrorKind::Quota
        );
        let pause = error
            .retry_after
            .or_else(|| limited.then(|| self.full_again_in()).flatten())
            .unwrap_or_else(|| match error.kind {
                ProviderErrorKind::Quota => QUOTA_PAUSE,
                _ => (Duration::from_millis(u64::try_from(waited).unwrap_or(0)) / 2)
                    .clamp(MIN_PAUSE, MAX_PAUSE),
            });
        // A provider that says "now" again and again must not spin the turn.
        pause.clamp(UNIT, MAX_AHEAD)
    }

    /// When the current provider's limits it reported used up are full again.
    fn full_again_in(&self) -> Option<Duration> {
        let choice = self.model.current()?;
        let config = self.config.current();
        let settings = config.providers().get(&choice.provider_name)?;
        full_again_in(&Account::of(settings).limits()?)
    }
}

/// The time until every limit that is used up is full again; none when none is, or the
/// provider did not say when.
fn full_again_in(limits: &RateLimits) -> Option<Duration> {
    let counted = [&limits.requests, &limits.tokens]
        .into_iter()
        .flatten()
        .filter(|limit| limit.limit > 0 && limit.remaining == 0)
        .map(|limit| limit.resets_in);
    let plan = limits
        .plan
        .iter()
        .filter(|window| window.used_percent >= 100)
        .map(|window| window.resets_in);
    counted.chain(plan).flatten().max().map(Duration::from_secs)
}

fn millis(duration: Duration) -> i64 {
    i64::try_from(duration.as_millis()).unwrap_or(i64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use dusk_protocol::model::{PlanWindow, RateLimit};

    #[test]
    fn only_used_up_limits_say_when_the_provider_is_back() {
        let limit = |remaining, resets_in| RateLimit {
            limit: 10,
            remaining,
            resets_in,
        };
        let mut limits = RateLimits {
            requests: Some(limit(3, Some(5))),
            tokens: Some(limit(0, Some(40))),
            plan: Vec::new(),
        };
        assert_eq!(full_again_in(&limits), Some(Duration::from_secs(40)));
        limits.tokens = Some(limit(1, Some(40)));
        assert_eq!(full_again_in(&limits), None);
        limits.plan.push(PlanWindow {
            minutes: Some(300),
            used_percent: 100,
            resets_in: Some(900),
        });
        assert_eq!(full_again_in(&limits), Some(Duration::from_secs(900)));
    }
}
