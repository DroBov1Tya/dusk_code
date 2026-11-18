//! What the running turn has used, as its working row shows it: the core counts the
//! steps and tokens, and every client draws the same fields from them.

use crate::thread::{ThreadEvent, Turn, short_count};

/// The running turn's totals, from the newest event that carried them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TurnSpend {
    /// Model calls that ended.
    pub steps: u32,
    pub tokens: u64,
    pub cost_micro_usd: u64,
}

impl TurnSpend {
    pub fn of(turn: &Turn) -> Self {
        Self {
            steps: turn.steps,
            tokens: turn.tokens,
            cost_micro_usd: turn.cost_micro_usd,
        }
    }

    /// The step under way: the one after those that ended, as "step 7".
    pub fn step(&self) -> String {
        format!("step {}", self.steps.saturating_add(1))
    }

    /// The tokens as "18.4k", or none before any call reported usage: the field is then
    /// absent, not zero.
    pub fn tokens(&self) -> Option<String> {
        (self.tokens > 0).then(|| short_count(self.tokens))
    }
}

/// What `event` says of the running turn's spend: `Some(Some(_))` sets it,
/// `Some(None)` clears it as the turn ended, `None` leaves it as it was.
pub fn turn_spend(event: &ThreadEvent) -> Option<Option<TurnSpend>> {
    Some(match event {
        ThreadEvent::TurnStarted(turn)
        | ThreadEvent::TurnRunning(turn)
        | ThreadEvent::TurnWaiting(turn) => Some(TurnSpend::of(turn)),
        ThreadEvent::TurnProgress {
            steps,
            tokens,
            cost_micro_usd,
            ..
        } => Some(TurnSpend {
            steps: *steps,
            tokens: *tokens,
            cost_micro_usd: *cost_micro_usd,
        }),
        ThreadEvent::TurnFinished(_) | ThreadEvent::Failed(_) | ThreadEvent::Idle => None,
        _ => return None,
    })
}
