//! A model that is stuck repeats itself. With no cap on steps, this is what stops it from
//! burning a turn: the result of a repeated call carries a note to change course. Two
//! rules share the note: the same call with the same result in a row (`same_call`), and,
//! when `limits.failure_ladder` is on, the same error through calls that differ
//! (`ladder`, with `signature` saying when two errors are the same). The same count tells,
//! with `limits.episodes` on, when a pass ended such an error (`solved`).

mod ladder;
mod same_call;
mod signature;
#[cfg(test)]
mod signature_tests;
mod solved;
#[cfg(test)]
mod solved_tests;
#[cfg(test)]
mod tests;

use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
};

use dusk_protocol::thread::RepeatRung;

use crate::{config::Limits, providers::ToolCall, tools::ToolResult};

pub use solved::{Solved, lesson, lesson_note};

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
    /// A result of this turn asked the model for a lesson line.
    lesson_asked: bool,
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
        self.enter(turn_id);
        let same = self.same_call.note(&call.name, &call.arguments, content);
        // With both switches off, nothing of the count runs: no signature, no cost.
        let counting = limits.failure_ladder || limits.episodes;
        let counted = (counting && result.is_error)
            .then(|| self.ladder.count(call, &result.content))
            .flatten();
        let climbed = counted
            .filter(|_| limits.failure_ladder)
            .and_then(|key| self.ladder.climb(key, limits));
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

    /// The loop a call that passed ended, for an episode. Only asked with
    /// `limits.episodes` on, which is what counts the failures it ends.
    pub fn solved(
        &mut self,
        turn_id: &str,
        call: &ToolCall,
        result: &ToolResult,
    ) -> Option<Solved> {
        self.enter(turn_id);
        self.ladder.solved(call, result)
    }

    /// Notes that a result of the turn asked for a lesson line.
    pub fn ask_lesson(&mut self, turn_id: &str) {
        self.enter(turn_id);
        self.lesson_asked = true;
    }

    /// Whether a result of the turn `turn_id` asked for a lesson line.
    pub fn lesson_asked(&self, turn_id: &str) -> bool {
        self.turn == turn_id && self.lesson_asked
    }

    /// A new turn starts every count over.
    fn enter(&mut self, turn_id: &str) {
        if self.turn != turn_id {
            *self = Self {
                turn: turn_id.to_owned(),
                ..Self::default()
            };
        }
    }

    /// Whether the turn `turn_id` must end once the call at hand is done, so the rest of
    /// its calls are not run.
    pub fn stopping(&self, turn_id: &str) -> bool {
        self.turn == turn_id && self.stop.is_some()
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
