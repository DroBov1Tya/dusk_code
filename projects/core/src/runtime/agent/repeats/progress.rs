//! Steps that bring nothing new. With no cap on steps, a turn can call tools for ever
//! without getting anywhere, each call a little different, so no rule about repeats sees
//! it. A step makes progress when one of its calls:
//!
//! - changed a project file to a state not seen in the turn (the change ledger's rows of
//!   the call, by path and content hash), so an edit undone and done again is no news;
//! - gave a result not seen in the turn (by `cycles::result_hash`, so times and ids
//!   that differ make no news), or, as a read, asked for something not asked before:
//!   a new file, range or search is research;
//! - passed as a command whose program failed earlier in the turn.
//!
//! Steps in a row without any of these are counted; at `limits.no_progress_note_at` the
//! model is asked where it stands, at `no_progress_replan_at` for a different plan, and
//! again every `no_progress_note_at` steps after. Notes only: the turn is marked a stall
//! candidate, for a judge to confirm, and never ended here.
//!
//! The ledger sees only this thread's foreground commands. Until it has recorded a change
//! in the turn, its silence proves nothing, so a call that may write and was not made
//! before counts as progress: a project too large to watch is never nagged for edits.
//! So does such a call where the ledger never looks: a subagent, a job, an MCP tool.
//!
//! A look at the background jobs is neither: a build or a run going on shows little more
//! than a counter between two looks, which is not news, yet the job is moving. A step of
//! nothing but such looks is not counted.

use std::collections::{HashSet, VecDeque};

use dusk_protocol::thread::RepeatRung;

use super::{hash, ladder, signature};
use crate::{config::Limits, providers::ToolCall, tools::ToolResult};

/// Hashes kept per kind in a turn; the oldest go first, so a turn of any length holds
/// a bounded set. Something seen this long ago is new enough again.
const MAX_SEEN: usize = 2048;
/// Programs a turn keeps as failing, to see one pass.
const MAX_FAILING: usize = 64;
/// The tool that reads and waits on background jobs.
const JOBS: &str = "jobs";

/// What one call did, as the caller knows it.
pub struct Call<'a> {
    pub call: &'a ToolCall,
    pub result: &'a ToolResult,
    pub call_hash: u64,
    pub result_hash: u64,
    pub read_only: bool,
    /// The ledger's rows for the call, as (path, state after); None when the ledger was
    /// not read for it.
    pub edits: Option<&'a [(String, String)]>,
}

#[derive(Default)]
struct Seen {
    set: HashSet<u64>,
    order: VecDeque<u64>,
}

impl Seen {
    /// Adds `value`; whether it was new.
    fn add(&mut self, value: u64) -> bool {
        if !self.set.insert(value) {
            return false;
        }
        if self.order.len() == MAX_SEEN
            && let Some(oldest) = self.order.pop_front()
        {
            self.set.remove(&oldest);
        }
        self.order.push_back(value);
        true
    }
}

#[derive(Default)]
pub struct Progress {
    results: Seen,
    calls: Seen,
    edits: Seen,
    /// Programs of commands that failed in the turn and have not passed since.
    failing: HashSet<(String, Option<String>)>,
    /// The ledger recorded a change of this turn: from then on a call it shows no rows
    /// for changed nothing.
    ledger_seen: bool,
    /// A step is running.
    started: bool,
    /// The running step brought something new so far.
    moved: bool,
    /// The running step made a call other than a look at the jobs.
    counted: bool,
    /// Finished steps in a row that brought nothing new.
    idle: u32,
    /// The count the next notice waits for; 0 before the first since progress.
    next_at: u32,
    /// The re-plan was asked for with no progress since and the judge has not been
    /// told yet.
    stall: bool,
}

impl Progress {
    /// A step starts: the one before is counted.
    pub fn step(&mut self) {
        if self.started && self.counted {
            if self.moved {
                self.idle = 0;
                self.next_at = 0;
                self.stall = false;
            } else {
                self.idle = self.idle.saturating_add(1);
            }
        }
        self.started = true;
        self.moved = false;
        self.counted = false;
    }

    /// Notes a finished call; the rung, count and note when the steps before brought
    /// nothing new and neither has the step so far.
    pub fn note(&mut self, done: &Call<'_>, limits: &Limits) -> Option<(RepeatRung, u32, String)> {
        if !limits.no_progress || done.call.name == JOBS {
            return None;
        }
        self.counted = true;
        if self.brings_news(done) {
            self.moved = true;
        }
        if self.moved {
            return None;
        }
        let at = match self.next_at {
            0 => limits.no_progress_note_at,
            at => at,
        };
        if self.idle < at {
            return None;
        }
        let count = self.idle;
        Some(if count >= limits.no_progress_replan_at {
            (RepeatRung::NoProgressReplan, count, replan(count))
        } else {
            (RepeatRung::NoProgress, count, note(count))
        })
    }

    /// The notice of `rung` went to the model: the next waits for more steps.
    pub fn noticed(&mut self, rung: RepeatRung, limits: &Limits) {
        match rung {
            RepeatRung::NoProgress => self.next_at = limits.no_progress_replan_at,
            RepeatRung::NoProgressReplan => {
                self.next_at = self.idle.saturating_add(limits.no_progress_note_at);
                self.stall = true;
            }
            _ => {}
        }
    }

    /// Steps in a row with nothing new, once the re-plan was asked for and nothing came
    /// of it; given once per re-plan.
    pub fn take_stall(&mut self) -> Option<u32> {
        std::mem::take(&mut self.stall).then_some(self.idle)
    }

    /// Whether the call brought anything new. Every set is fed, so what a call shows
    /// once is known the next time.
    fn brings_news(&mut self, done: &Call<'_>) -> bool {
        let result = self.results.add(done.result_hash);
        let asked = self.calls.add(done.call_hash);
        let mut edited = false;
        if let Some(edits) = done.edits.filter(|edits| !edits.is_empty()) {
            self.ledger_seen = true;
            for edit in edits {
                edited |= self.edits.add(hash(edit));
            }
        }
        let passed = self.passed(done.call, done.result);
        let unseen_write = !done.read_only && (!self.ledger_seen || ledger_blind(done));
        result || edited || passed || (asked && (done.read_only || unseen_write))
    }

    /// Whether `call` passed as a command whose program failed earlier in the turn; a
    /// failure is kept for later.
    fn passed(&mut self, call: &ToolCall, result: &ToolResult) -> bool {
        let Some(program) = ladder::invocation(call) else {
            return false;
        };
        if result.is_error {
            if self.failing.len() < MAX_FAILING {
                self.failing.insert(program);
            }
            return false;
        }
        result.exit_code == Some(0) && self.failing.remove(&program)
    }
}

/// Whether the call may write where the ledger does not look: it records only what a
/// foreground command of this thread changed, not a subagent, a job or an MCP tool.
fn ledger_blind(done: &Call<'_>) -> bool {
    let foreground = done.call.name == signature::RUN_COMMAND
        && done.call.arguments["background"].as_bool() != Some(true)
        && done.result.exit_code.is_some();
    !foreground
}

fn note(count: u32) -> String {
    format!(
        "[your last {count} steps brought nothing new: no file changed, nothing new read, \
         nothing that failed passed. In one line, say what you have found and what you will \
         do next, then do it.]"
    )
}

fn replan(count: u32) -> String {
    format!(
        "[{count} steps in a row have brought nothing new; the approach is not working. \
         Before your next call, write one sentence with a different plan, then follow it, \
         or ask the user.]"
    )
}
