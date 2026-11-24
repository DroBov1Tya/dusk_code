//! A model that is stuck repeats itself. With no cap on steps, this is what stops it from
//! burning a turn: the result of a repeated call carries a note to change course. The
//! rules share the note:
//!
//! - calls going in circles (`cycles`): the same call with the same result in a row, a
//!   short run of calls coming round again, or a read giving the same result with
//!   nothing changed between; with `limits.loop_cycles` on they escalate to a re-plan;
//! - tests run again with no project file changed (`test_runs`, `limits.test_reruns`);
//! - steps in a row that bring nothing new (`progress`, `limits.no_progress`), which also
//!   mark the turn a stall candidate for a judge, as a cycle asked twice for a different
//!   plan does (`judge` has what the judge reads and how its answer is taken);
//! - the model's text going round, one piece written again and again (`chant`,
//!   `limits.text_loops`), read as it streams so the answer is cut short;
//! - when `limits.failure_ladder` is on, the same error through calls that differ
//!   (`ladder`, with `signature` saying when two errors are the same, and a test run's
//!   failing ids standing for its error). The same count tells, with `limits.episodes`
//!   on, when a pass ended such an error (`solved`).
//!
//! Only the ladder ends a turn by itself; the other rules add notes, and a stall candidate
//! ends it only when the judge is sure it is stuck. The test results of the turn,
//! and whether it wrote where the change ledger does not look, are kept too, for a goal
//! to tell whether its rounds still change anything (`Round`).

mod chant;
#[cfg(test)]
mod chant_tests;
mod cycles;
#[cfg(test)]
mod cycles_tests;
mod judge;
#[cfg(test)]
mod judge_tests;
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
    collections::{BTreeMap, VecDeque, hash_map::DefaultHasher},
    hash::{Hash, Hasher},
};

use dusk_protocol::thread::RepeatRung;

use crate::{config::Limits, providers::ToolCall, tools::ToolResult};

pub use chant::{Chant, note as chant_note};
pub use judge::{Verdict, digest, judge_note, judge_stop, verdict};
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
pub struct Facts {
    /// The call only reads: it can change nothing.
    pub read_only: bool,
    /// The call may change a project file (`Tool::writes_project`): not a read, nor a
    /// harness tool that keeps the chat's own records.
    pub writes_project: bool,
    /// The call ran a command whose changes the ledger could not record
    /// (`State::note_unwatched`).
    pub unwatched: bool,
    /// The change ledger's count for the thread after the call; read only for a test run
    /// (`is_test_run`) with `limits.test_reruns` and `limits.record_changes` on.
    pub ledger: Option<u64>,
    /// The ledger's rows for the call, as (path, state after); read only for a call that
    /// may write, with `limits.no_progress` and `limits.record_changes` on.
    pub edits: Option<Vec<(String, String)>>,
}

/// A call that may write anywhere, as an unknown tool may.
impl Default for Facts {
    fn default() -> Self {
        Self {
            read_only: false,
            writes_project: true,
            unwatched: false,
            ledger: None,
            edits: None,
        }
    }
}

/// How a test runner's last run in a turn ended.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TestOutcome {
    /// Whether it passed and which tests failed, for the user.
    pub shown: String,
    /// The run's error lines, made alike as `signature::volatile` does; empty when it
    /// passed or wrote none.
    pub error: String,
}

/// What a turn did that a goal round is measured by.
#[derive(Debug, Default)]
pub struct Round {
    /// The last result of each test runner, sorted by runner.
    pub tests: Vec<TestOutcome>,
    /// A call may have written where the change ledger does not look: a background job,
    /// a subagent, an MCP tool.
    pub unseen_writes: bool,
}

/// A turn a judge should read: a rule asked for a different plan and nothing came of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stall {
    /// What the rule counted: steps with nothing new, or rounds of a cycle.
    pub count: u32,
    /// What the rule saw, in a few words, for the judge and the user.
    pub what: String,
}

/// What the judge reads of the turn besides the user's request.
#[derive(Debug, Default)]
pub struct Digest {
    /// The newest calls of the turn, one line each, oldest first.
    pub calls: Vec<String>,
    /// The error that came back most often, with its attempts, when one came back.
    pub error: Option<String>,
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
    /// The last result of each test runner in the turn, at most `MAX_TEST_RUNNERS`.
    tests: BTreeMap<String, TestOutcome>,
    ladder: ladder::Ladder,
    /// Why the turn must end, once the ladder reached its top.
    stop: Option<String>,
    /// A result of this turn asked the model for a lesson line.
    lesson_asked: bool,
    /// The newest calls, one line each, for a judge; kept with `limits.loop_judge` on.
    recent: VecDeque<String>,
    /// Steps whose calls the cycle rule asked a re-plan of, in a row with the cycle
    /// going on and nothing new, since it last marked a stall.
    cycle_replans: u32,
    /// The step the newest of those re-plans was asked in.
    cycle_replan_step: u32,
    /// A cycle asked twice for a different plan and the judge has not been told yet.
    cycle_stall: Option<Stall>,
    /// The same failing tests reached the ladder's top and the judge has not been told.
    tests_stall: Option<Stall>,
    /// Judge calls made in the turn.
    judged: u32,
    /// Stall candidates to let pass before the next judge call: each judge that let
    /// the turn go on makes the next one wait longer.
    skip: u32,
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
                writes_project: facts.writes_project,
                unwatched: facts.unwatched,
                edits: facts.edits.as_deref(),
            },
            limits,
        );
        let tests = (limits.test_reruns || limits.goal_stall)
            .then(|| test_runs::TestRun::of(call, result))
            .flatten();
        if let Some(run) = &tests
            && limits.goal_stall
        {
            self.keep_test_result(run, call, result);
        }
        let tests = tests.filter(|_| limits.test_reruns);
        let signature = tests.as_ref().and_then(test_runs::TestRun::signature);
        // The same failing tests come back through real edits when some fail for another
        // reason: at the ladder's top the judge reads the turn. Where no judge can rule
        // (it is off, or its calls are spent) the old stop stands, twice as far once
        // judges let the turn go on, so a stuck run of tests still ends.
        let by_ids = signature.is_some();
        let stop_at = if !by_ids || !limits.loop_judge {
            Some(limits.failure_stop_at)
        } else if self.judged >= limits.loop_judge_calls {
            Some(limits.failure_stop_at.saturating_mul(2))
        } else {
            None
        };
        let rerun = match tests {
            Some(run) => self.test_runs.run(self.steps, run, facts.ledger, limits),
            None => {
                self.test_runs.other(call, result, facts);
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
            .and_then(|key| self.ladder.climb(key, limits, stop_at));
        if limits.loop_judge {
            self.keep_recent(call, result);
        }
        let mut notices = Vec::new();
        if let Some(climbed) = climbed {
            if climbed.stop.is_some() {
                self.stop = climbed.stop;
            }
            if by_ids && climbed.count >= limits.failure_stop_at {
                self.tests_stall = Some(Stall {
                    count: climbed.count,
                    what: format!("the same tests failed {} times in this turn", climbed.count),
                });
            }
            notices.push((climbed.rung, climbed.count, climbed.note));
        }
        // A call outside the cycle, or one that brought something new, ends the run of
        // re-plans: another cycle later starts its own.
        if cycle.is_none() || self.progress.moved() {
            self.cycle_replans = 0;
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
        // Every call of a step past the mark is asked again: the step counts once, as
        // the model reads the re-plan only after it.
        if notice.rung == RepeatRung::CycleReplan
            && (self.cycle_replans == 0 || self.cycle_replan_step != self.steps)
        {
            self.cycle_replan_step = self.steps;
            self.cycle_replans += 1;
            if self.cycle_replans >= CYCLE_REPLANS_TO_JUDGE {
                self.cycle_replans = 0;
                self.cycle_stall = Some(Stall {
                    count: notice.count,
                    what: format!(
                        "the same calls came round {} times with the same results",
                        notice.count
                    ),
                });
            }
        }
        Some(notice)
    }

    fn keep_recent(&mut self, call: &ToolCall, result: &ToolResult) {
        let outcome = match (result.is_error, result.exit_code) {
            (false, _) => "ok".to_owned(),
            (true, Some(code)) => format!("failed, exit {code}"),
            (true, None) => "failed".to_owned(),
        };
        if self.recent.len() >= judge::RECENT_CALLS {
            self.recent.pop_front();
        }
        let shown = ladder::Attempt::of(call).shown;
        self.recent.push_back(format!("{shown} -> {outcome}"));
    }

    fn keep_test_result(&mut self, run: &test_runs::TestRun, call: &ToolCall, result: &ToolResult) {
        if self.tests.len() >= MAX_TEST_RUNNERS && !self.tests.contains_key(&run.runner) {
            return;
        }
        let (shown, error) = match run.exit_code {
            0 => ("passed".to_owned(), String::new()),
            code => {
                // A run that fails to build names no failing test: its error lines, with
                // line numbers and times made alike but counts kept, tell two apart.
                let error = signature::of(&call.name, &call.arguments, &result.content)
                    .map(|found| {
                        let lines: Vec<String> =
                            found.error.lines().map(signature::volatile).collect();
                        lines.join("\n")
                    })
                    .unwrap_or_default();
                (format!("exit {code}: {}", run.failing.join(" ")), error)
            }
        };
        self.tests
            .insert(run.runner.clone(), TestOutcome { shown, error });
    }

    /// What the turn `turn_id` did that a goal round is measured by; empty when it called
    /// no tool or is not the turn these counts are of.
    pub fn round(&self, turn_id: &str) -> Round {
        if self.turn != turn_id {
            return Round::default();
        }
        Round {
            tests: self
                .tests
                .iter()
                .map(|(runner, outcome)| TestOutcome {
                    shown: format!("{runner}: {}", outcome.shown),
                    error: outcome.error.clone(),
                })
                .collect(),
            unseen_writes: self.test_runs.unseen_writes(),
        }
    }

    /// The turn `turn_id` as a stall candidate, once a rule asked for a different plan
    /// and nothing came of it: steps in a row with nothing new past the re-plan, a
    /// cycle asked twice, or the same failing tests at the ladder's top. Given once per
    /// mark, and only while the judge is on and has calls left; marks that come while a
    /// judge's redirect is being given time pass.
    pub fn take_stall(&mut self, turn_id: &str, limits: &Limits) -> Option<Stall> {
        if self.turn != turn_id {
            return None;
        }
        let idle = self.progress.take_stall().map(|idle| Stall {
            count: idle,
            what: format!("{idle} steps in a row brought nothing new"),
        });
        let stall = self
            .cycle_stall
            .take()
            .or_else(|| self.tests_stall.take())
            .or(idle)?;
        if !limits.loop_judge || self.judged >= limits.loop_judge_calls {
            return None;
        }
        if self.skip > 0 {
            self.skip -= 1;
            return None;
        }
        Some(stall)
    }

    /// What a judge of the turn `turn_id` reads of it.
    pub fn digest(&self, turn_id: &str) -> Digest {
        if self.turn != turn_id {
            return Digest::default();
        }
        Digest {
            calls: self.recent.iter().cloned().collect(),
            error: self.ladder.worst(),
        }
    }

    /// A judge read the turn `turn_id` and let it go on, or could not answer: the next
    /// stall candidate it reads waits for one more mark than this one did.
    pub fn judged(&mut self, turn_id: &str) {
        self.enter(turn_id);
        self.judged = self.judged.saturating_add(1);
        self.skip = self.judged;
    }

    /// Whether the turn `turn_id` called any tool.
    pub fn acted(&self, turn_id: &str) -> bool {
        self.turn == turn_id && self.steps > 0
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
/// Re-plans a cycle is asked for before a judge reads the turn: the first may still work.
const CYCLE_REPLANS_TO_JUDGE: u32 = 2;

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
