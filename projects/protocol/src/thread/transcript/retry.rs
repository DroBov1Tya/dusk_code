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
        | ThreadEvent::TurnWaiting(_)
        | ThreadEvent::TurnFailed(_)
        | ThreadEvent::TurnFinished(_)
        | ThreadEvent::CompactionFinished(_)
        | ThreadEvent::CompactionFailed { .. }
        | ThreadEvent::Failed(_)
        | ThreadEvent::Idle => RetryRow::Clear,
        _ => return None,
    })
}
