//! A model that is stuck repeats itself. With no cap on steps, this is what stops it from
//! burning a turn: the result of a repeated call carries a note to change course. The
//! rules share the note:
//!
//! - calls going in circles (`cycles`): the same call with the same result in a row, a
//!   short run of calls coming round again, or a read giving the same result with
//!   nothing changed between; with `limits.loop_cycles` on they escalate to a re-plan;
//! - tests run again with no project file changed (`test_runs`, `limits.test_reruns`);
//! - steps in a row that bring nothing new (`progress`, `limits.no_progress`), which also
//!   mark the turn a stall candidate for a judge;
//! - when `limits.failure_ladder` is on, the same error through calls that differ
//!   (`ladder`, with `signature` saying when two errors are the same, and a test run's
//!   failing ids standing for its error). The same count tells, with `limits.episodes`
//!   on, when a pass ended such an error (`solved`).
//!
//! Only the ladder ends a turn; the other rules add notes. The test results of the turn
//! are kept too, for a goal to tell whether its rounds still change anything.

mod cycles;
#[cfg(test)]
mod cycles_tests;
mod ladder;
mod progress;
#[cfg(test)]
mod progress_tests;
mod signature;
#[cfg(test)]
mod signature_tests;
mod solved;
#[cfg(test)]
mod solved_tests;
mod test_runs;
#[cfg(test)]
mod test_runs_tests;
#[cfg(test)]
mod tests;

use std::{
    collections::{BTreeMap, hash_map::DefaultHasher},
    hash::{Hash, Hasher},
};

use dusk_protocol::thread::RepeatRung;

use crate::{config::Limits, providers::ToolCall, tools::ToolResult};

pub use solved::{Solved, lesson, lesson_note};
pub use test_runs::is_test_run;

/// What the model is told about a repeated call, and what clients and evals are told.
pub struct Notice {
    pub rung: RepeatRung,
    pub count: u32,
    /// Added to the result the model reads.
    pub note: String,
}

/// What the caller knows of a call that the rules cannot see in it.
#[derive(Default)]
pub struct Facts {
    /// The call only reads: it can change nothing.
    pub read_only: bool,
    /// The change ledger's count for the thread after the call; read only for a test run
    /// (`is_test_run`) with `limits.test_reruns` and `limits.record_changes` on.
    pub ledger: Option<u64>,
    /// The ledger's rows for the call, as (path, state after); read only for a call that
    /// may write, with `limits.no_progress` and `limits.record_changes` on.
    pub edits: Option<Vec<(String, String)>>,
}

/// Kept per turn: a new turn starts every count over.
#[derive(Default)]
pub struct Repeats {
    turn: String,
    /// Steps of the turn that called tools, the one running included.
    steps: u32,
    cycles: cycles::Cycles,
    test_runs: test_runs::TestRuns,
    progress: progress::Progress,
    /// The last result of each test runner in the turn: whether it passed and what
    /// failed, at most `MAX_TEST_RUNNERS`.
    tests: BTreeMap<String, String>,
    ladder: ladder::Ladder,
    /// Why the turn must end, once the ladder reached its top.
    stop: Option<String>,
    /// A result of this turn asked the model for a lesson line.
    lesson_asked: bool,
}

impl Repeats {
    /// Notes that a step of the turn starts running its calls.
    pub fn step(&mut self, turn_id: &str) {
        self.enter(turn_id);
        self.steps = self.steps.saturating_add(1);
        self.progress.step();
    }

    /// Notes a call that ran; `content` is what the model will read of its result. When
    /// more than one rule applies, the one that asks for most is given: a stop, then a
    /// re-plan, then a note, and of two notes the more specific.
    pub fn note(
        &mut self,
        turn_id: &str,
        call: &ToolCall,
        result: &ToolResult,
        content: &str,
        facts: &Facts,
        limits: &Limits,
    ) -> Option<Notice> {
        self.enter(turn_id);
        let call_hash = hash((&call.name, call.arguments.to_string()));
        // Progress compares results with times and ids made alike too, or every test run
        // would bring news.
        let alike =
            (limits.loop_cycles || limits.no_progress).then(|| cycles::result_hash(content));
        let result_hash = match alike {
            Some(alike) if limits.loop_cycles => alike,
            _ => hash(content),
        };
        let cycle = self
            .cycles
            .note(self.steps, facts.read_only, call_hash, result_hash, limits);
        let moved = self.progress.note(
            &progress::Call {
                call,
                result,
                call_hash,
                result_hash: alike.unwrap_or(result_hash),
                read_only: facts.read_only,
                edits: facts.edits.as_deref(),
            },
            limits,
        );
        let tests = (limits.test_reruns || limits.goal_stall)
            .then(|| test_runs::TestRun::of(call, result))
            .flatten();
        if let Some(run) = &tests {
            self.keep_test_result(run);
        }
        let tests = tests.filter(|_| limits.test_reruns);
        let signature = tests.as_ref().and_then(test_runs::TestRun::signature);
        let rerun = match tests {
            Some(run) => self.test_runs.run(self.steps, run, facts.ledger, limits),
            None => {
                self.test_runs.other(call, result, facts.read_only);
                None
            }
        };
        // With both switches off, nothing of the count runs: no signature, no cost.
        let counting = limits.failure_ladder || limits.episodes;
        let counted = (counting && result.is_error)
            .then(|| match signature {
                Some(signature) => self.ladder.count_signature(call, signature),
                None => self.ladder.count(call, &result.content),
            })
            .flatten();
        let climbed = counted
            .filter(|_| limits.failure_ladder)
            .and_then(|key| self.ladder.climb(key, limits));
        let mut notices = Vec::new();
        if let Some(climbed) = climbed {
            if climbed.stop.is_some() {
                self.stop = climbed.stop;
            }
            notices.push((climbed.rung, climbed.count, climbed.note));
        }
        notices.extend(rerun);
        notices.extend(cycle);
        notices.extend(moved);
        let notice = notices
            .into_iter()
            .min_by_key(|(rung, ..)| weight(*rung))
            .map(|(rung, count, note)| Notice { rung, count, note })?;
        // A no-progress note that lost to another rule's comes with a later call.
        self.progress.noticed(notice.rung, limits);
        Some(notice)
    }

    fn keep_test_result(&mut self, run: &test_runs::TestRun) {
        if self.tests.len() >= MAX_TEST_RUNNERS && !self.tests.contains_key(&run.runner) {
            return;
        }
        let outcome = match run.exit_code {
            0 => "passed".to_owned(),
            code => format!("exit {code}: {}", run.failing.join(" ")),
        };
        self.tests.insert(run.runner.clone(), outcome);
    }

    /// The last result of each test runner in the turn `turn_id`, one line each, sorted;
    /// empty when it ran none or is not the turn these counts are of.
    pub fn test_results(&self, turn_id: &str) -> Vec<String> {
        if self.turn != turn_id {
            return Vec::new();
        }
        self.tests
            .iter()
            .map(|(runner, outcome)| format!("{runner}: {outcome}"))
            .collect()
    }

    /// Steps in a row with nothing new in the turn `turn_id`, once the model was asked
    /// for a different plan for that: a turn a judge should look at. Given once per
    /// re-plan; None after progress.
    pub fn take_stall(&mut self, turn_id: &str) -> Option<u32> {
        if self.turn != turn_id {
            return None;
        }
        self.progress.take_stall()
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

/// Test runners a turn keeps the last result of.
const MAX_TEST_RUNNERS: usize = 16;

/// Which notice wins when rules agree on a call: lower first.
fn weight(rung: RepeatRung) -> u8 {
    match rung {
        RepeatRung::Stop | RepeatRung::JudgeStop => 0,
        RepeatRung::Replan => 1,
        RepeatRung::TestRerunReplan => 2,
        RepeatRung::CycleReplan | RepeatRung::NoProgressReplan => 3,
        RepeatRung::TestRerun => 4,
        RepeatRung::SameError => 5,
        RepeatRung::Cycle | RepeatRung::NoProgress | RepeatRung::JudgeRedirect => 6,
        RepeatRung::SameCall => 7,
    }
}

fn hash(value: impl Hash) -> u64 {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}
