//! A model call being tried again: one row per call, which each `Retrying` replaces and
//! which goes once the call answers or the turn ends, so a chat never grows a list of
//! attempts.

use crate::model::ProviderErrorKind;
use crate::thread::ThreadEvent;

/// The retry row of a chat, from the newest `Retrying` event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Retry {
    /// The attempt that failed, from 1.
    pub attempt: u32,
    /// None when the call is tried until a time budget runs out.
    pub max_attempts: Option<u32>,
    pub kind: ProviderErrorKind,
    /// The pause before the next attempt, counted from when the event arrived.
    pub wait_ms: u64,
    /// Safe to show.
    pub message: String,
}

impl Retry {
    /// The attempt that follows the pause: "attempt 3 of 6", or "attempt 9" under a time
    /// budget.
    pub fn next_attempt(&self) -> String {
        let next = self.attempt.saturating_add(1);
        match self.max_attempts {
            Some(max) => format!("attempt {next} of {max}"),
            None => format!("attempt {next}"),
        }
    }

    /// The row in one line, without the countdown a client adds: "Provider unreachable:
    /// could not reach api.example.com; trying again, attempt 2 of 6".
    pub fn text(&self) -> String {
        format!(
            "{}: {}; trying again, {}",
            self.kind.label(),
            self.message,
            self.next_attempt()
        )
    }
}

impl Retry {
    /// The time left before the next attempt, `elapsed_ms` after the event arrived:
    /// "next in 8 s", rounded up, or "trying now".
    pub fn countdown(&self, elapsed_ms: u64) -> String {
        match self.wait_ms.saturating_sub(elapsed_ms) {
            0 => "trying now".to_owned(),
            left => format!("next in {} s", left.div_ceil(1000)),
        }
    }

    /// The whole row: `text()` and the countdown.
    pub fn line(&self, elapsed_ms: u64) -> String {
        format!("{} \u{00B7} {}", self.text(), self.countdown(elapsed_ms))
    }
}

/// What an event does to the retry row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RetryRow {
    /// Show this in place of the row there is, or as a new one.
    Show(Retry),
    /// The call answered, or the turn or compaction ended: the row goes.
    Clear,
}

/// How `event` changes the retry row, if it does. The first `Model` event of a call is
/// the sign that it answered: no separate event says a retry passed.
pub fn retry_row(event: &ThreadEvent) -> Option<RetryRow> {
    Some(match event {
        ThreadEvent::Retrying {
            attempt,
            error,
            kind,
            wait_ms,
            max_attempts,
        } => RetryRow::Show(Retry {
            attempt: *attempt,
            max_attempts: *max_attempts,
            kind: *kind,
            wait_ms: *wait_ms,
            message: error.clone(),
        }),
        ThreadEvent::Model(_)
        | ThreadEvent::TurnStarted(_)
        | ThreadEvent::TurnFailed(_)
        | ThreadEvent::TurnFinished(_)
        | ThreadEvent::CompactionFinished(_)
        | ThreadEvent::CompactionFailed { .. }
        | ThreadEvent::Failed(_)
        | ThreadEvent::Idle => RetryRow::Clear,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn retry() -> Retry {
        Retry {
            attempt: 2,
            max_attempts: Some(6),
            kind: ProviderErrorKind::Overloaded,
            wait_ms: 8000,
            message: "busy".into(),
        }
    }

    #[test]
    fn the_countdown_rounds_up_and_ends_in_trying_now() {
        assert_eq!(retry().countdown(0), "next in 8 s");
        assert_eq!(retry().countdown(7_500), "next in 1 s");
        assert_eq!(retry().countdown(8_000), "trying now");
        assert_eq!(retry().countdown(99_000), "trying now");
        assert!(
            retry()
                .line(0)
                .ends_with("attempt 3 of 6 \u{00B7} next in 8 s")
        );
    }
}
