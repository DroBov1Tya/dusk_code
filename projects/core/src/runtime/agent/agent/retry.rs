//! How often a model call that failed in a way that may pass is tried again, and how
//! long each pause before the next attempt is.

use std::time::Duration;

use crate::{config::Limits, providers::ProviderErrorKind};

/// What the pauses and the time budget count in. Tests wait a millisecond for a second,
/// so a five-minute budget is run through in their time; the database they write to does
/// not suit tokio's paused clock.
#[cfg(not(test))]
const UNIT: Duration = Duration::from_secs(1);
#[cfg(test)]
const UNIT: Duration = Duration::from_millis(1);

/// Doubles before each further attempt, up to `MAX_RETRY_DELAY`.
const FIRST_RETRY_DELAY: Duration = UNIT;

const MAX_RETRY_DELAY: Duration = UNIT.saturating_mul(30);

/// What ends the attempts of a model call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Bound {
    /// This many attempts, the first included.
    Attempts(u32),
    /// Attempts until this long after the first began. A host that cannot be reached, or
    /// does not answer, often comes back within minutes, and a turn should outlast that.
    Time(Duration),
}

impl Bound {
    /// The bound for a failure of `kind`. One attempt set in the limits means no retries
    /// of any kind.
    pub(super) fn of(kind: ProviderErrorKind, limits: &Limits) -> Self {
        let lost = matches!(
            kind,
            ProviderErrorKind::Unreachable | ProviderErrorKind::TimedOut
        );
        match lost && limits.max_attempts > 1 {
            true => Self::Time(UNIT.saturating_mul(limits.retry_unreachable_seconds)),
            false => Self::Attempts(limits.max_attempts),
        }
    }

    /// Whether `attempt`, which failed `elapsed` after the first attempt began, is
    /// followed by another after `pause`.
    pub(super) fn allows(self, attempt: u32, elapsed: Duration, pause: Duration) -> bool {
        match self {
            Self::Attempts(max) => attempt < max,
            Self::Time(budget) => elapsed.saturating_add(pause) <= budget,
        }
    }

    /// What a client is told of it: the number of attempts, or none for a time budget.
    pub(super) fn max_attempts(self) -> Option<u32> {
        match self {
            Self::Attempts(max) => Some(max),
            Self::Time(_) => None,
        }
    }
}

/// Exponential backoff with up to a quarter of jitter, so agents that failed together do
/// not all come back at the same moment.
pub(super) fn retry_delay(attempt: u32) -> Duration {
    let base = FIRST_RETRY_DELAY
        .saturating_mul(2u32.saturating_pow(attempt.saturating_sub(1)))
        .min(MAX_RETRY_DELAY);
    // Without randomness the wait is only exact, which is still a wait.
    let noise = getrandom::u32().unwrap_or(0);
    base + base.mul_f64(f64::from(noise % 1000) / 4000.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_lost_host_is_tried_for_a_time_and_other_failures_a_number_of_times() {
        let limits = Limits::default();
        let lost = Bound::of(ProviderErrorKind::Unreachable, &limits);
        assert_eq!(lost, Bound::Time(UNIT * 300));
        assert_eq!(lost.max_attempts(), None);
        let second = UNIT;
        assert!(lost.allows(40, UNIT * 299, second));
        assert!(!lost.allows(2, UNIT * 300, second));
        let busy = Bound::of(ProviderErrorKind::Overloaded, &limits);
        assert_eq!(busy.max_attempts(), Some(6));
        assert!(busy.allows(5, Duration::ZERO, second));
        assert!(!busy.allows(6, Duration::ZERO, second));
        let mut once = limits;
        once.max_attempts = 1;
        let off = Bound::of(ProviderErrorKind::TimedOut, &once);
        assert!(!off.allows(1, Duration::ZERO, Duration::ZERO));
    }

    #[test]
    fn the_pause_doubles_up_to_its_cap() {
        for (attempt, base) in [(1, 1), (2, 2), (5, 16), (6, 30), (40, 30)] {
            let pause = retry_delay(attempt);
            let base = UNIT * base;
            assert!(
                pause >= base && pause <= base.mul_f64(1.25),
                "{attempt}: {pause:?}"
            );
        }
    }
}
