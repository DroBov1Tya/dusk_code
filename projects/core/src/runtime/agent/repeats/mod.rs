//! A model that is stuck repeats itself. With no cap on steps, this is what stops it from
//! burning a turn: the result of a repeated call carries a note to change course. Two
//! rules share the note: the same call with the same result in a row (`same_call`), and,
//! when `limits.failure_ladder` is on, the same error through calls that differ
//! (`ladder`, with `signature` saying when two errors are the same).

mod ladder;
mod same_call;
mod signature;
#[cfg(test)]
mod signature_tests;
#[cfg(test)]
mod tests;

use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
};

use dusk_protocol::thread::RepeatRung;

use crate::{config::Limits, providers::ToolCall, tools::ToolResult};

/// What the model is told about a repeated call, and what clients and evals are told.
pub struct Notice {
    pub rung: RepeatRung,
    pub count: u32,
    /// Added to the result the model reads.
    pub note: String,
}

/// Kept per turn: a new turn starts every count over.
#[derive(Default)]
pub struct Repeats {
    turn: String,
    same_call: same_call::SameCall,
    ladder: ladder::Ladder,
    /// Why the turn must end, once the ladder reached its top.
    stop: Option<String>,
}

impl Repeats {
    /// Notes a call that ran; `content` is what the model will read of its result. When
    /// both rules apply, the ladder's note says more and is the one given.
    pub fn note(
        &mut self,
        turn_id: &str,
        call: &ToolCall,
        result: &ToolResult,
        content: &str,
        limits: &Limits,
    ) -> Option<Notice> {
        if self.turn != turn_id {
            *self = Self {
                turn: turn_id.to_owned(),
                ..Self::default()
            };
        }
        let same = self.same_call.note(&call.name, &call.arguments, content);
        // With the switch off, nothing of the ladder runs: no signature, no cost.
        let climbed = (limits.failure_ladder && result.is_error)
            .then(|| self.ladder.climb(call, &result.content, limits))
            .flatten();
        if let Some(climbed) = climbed {
            if climbed.stop.is_some() {
                self.stop = climbed.stop;
            }
            return Some(Notice {
                rung: climbed.rung,
                count: climbed.count,
                note: climbed.note,
            });
        }
        same.map(|(count, note)| Notice {
            rung: RepeatRung::SameCall,
            count,
            note,
        })
    }

    /// Why the turn `turn_id` must end now, once; None while it may go on.
    pub fn take_stop(&mut self, turn_id: &str) -> Option<String> {
        if self.turn != turn_id {
            return None;
        }
        self.stop.take()
    }
}

fn hash(value: impl Hash) -> u64 {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}
