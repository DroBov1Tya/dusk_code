//! Calls going in circles. The last `WINDOW` calls of a turn are kept as (call, result)
//! hashes, and two shapes are looked for:
//!
//! - a short run of calls whose results come round again: `A,A,A` (the same call in a
//!   row), `A,B,A,B`, up to runs of `MAX_PERIOD` calls;
//! - one read-only call giving the same result again and again within the window, with
//!   nothing between that could have changed it.
//!
//! The read-only calls of one step run together and count as one entry, so a step that
//! reads five files is one thing done, not five; the same read twice in a row within a
//! step is still two. Results are compared with what changes
//! from run to run (ids, times, durations) made alike, so a timestamp does not hide a
//! repeat; a call or result that differs in anything else is new work and resets nothing
//! it should not. With `limits.loop_cycles` off only the oldest rule stays as it was:
//! the third call in a row identical to the one before, with a result identical to the
//! byte, gets a note; nothing is folded or made alike.
//!
//! Polling a background job is not noted here: a look at the jobs, or a plain wait once
//! one started, gives the same result until the job ends, and waiting is what it should
//! do. The caller leaves such calls out, as the no-progress rule tells them.
//!
//! A read seen again counts only the times within the steps whose results the model
//! still has whole (`limits.stub_keep_steps` with `limits.stub_results` on): reading a
//! file again because its result was put away is what the stub asks for.

use std::collections::VecDeque;

use dusk_protocol::thread::RepeatRung;

use super::{hash, signature};
use crate::config::Limits;

/// Calls kept, newest last: the window OpenHands looks back over.
pub const WINDOW: usize = 20;
/// The longest run of calls that can make a cycle.
const MAX_PERIOD: usize = 4;
/// A result is read this far from each end: enough to tell two apart, bounded for any size.
const RESULT_BYTES: usize = 4096;
/// The rule before cycles: identical calls in a row before the note.
const LEGACY_NOTE_AT: u32 = 3;

#[derive(Clone, Copy)]
struct Entry {
    call: u64,
    result: u64,
    /// The step of the turn it ran in, to fold the reads of one step together.
    step: u32,
    read: bool,
    /// Calls that may write which ran before it.
    writes: u64,
    /// The last call folded in and its result, so the same read twice in one step is
    /// two entries: asking for one thing again is a repeat even in parallel.
    newest: (u64, u64),
}

impl Entry {
    fn same(&self, other: &Entry) -> bool {
        self.call == other.call && self.result == other.result
    }
}

#[derive(Default)]
pub struct Cycles {
    ring: VecDeque<Entry>,
    writes: u64,
    /// The last call and its raw result, and how many times in a row, for the rule kept
    /// with `limits.loop_cycles` off.
    legacy: Option<((u64, u64), u32)>,
}

/// What came round: the same call in a row, a run of calls, or a read again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Shape {
    InARow,
    Period(usize),
    Again,
}

impl Cycles {
    /// Notes a finished call by its hash and its result's (`result_hash` with
    /// `limits.loop_cycles` on, the raw text's hash off); the rung, count and note when it
    /// goes in circles.
    pub fn note(
        &mut self,
        step: u32,
        read_only: bool,
        call: u64,
        result: u64,
        limits: &Limits,
    ) -> Option<(RepeatRung, u32, String)> {
        if !limits.loop_cycles {
            let count = match self.legacy {
                Some((last, count)) if last == (call, result) => count.saturating_add(1),
                _ => 1,
            };
            self.legacy = Some(((call, result), count));
            return (count >= LEGACY_NOTE_AT)
                .then(|| (RepeatRung::SameCall, count, note(Shape::InARow, count)));
        }
        self.push(step, read_only, call, result);
        let kept = limits.stub_results.then_some(limits.stub_keep_steps);
        let (shape, count) = match (self.period(), self.again(kept)) {
            (Some(period), Some(again)) if again.1 > period.1 => again,
            (Some(period), _) => period,
            (None, again) => again?,
        };
        if count >= limits.cycle_replan_at {
            Some((RepeatRung::CycleReplan, count, replan(shape, count)))
        } else if count >= limits.cycle_note_at {
            let rung = match shape {
                Shape::InARow => RepeatRung::SameCall,
                _ => RepeatRung::Cycle,
            };
            Some((rung, count, note(shape, count)))
        } else {
            None
        }
    }

    fn push(&mut self, step: u32, read: bool, call: u64, result: u64) {
        match self.ring.back_mut() {
            Some(last)
                if read && last.read && last.step == step && last.newest != (call, result) =>
            {
                last.call = hash((last.call, call));
                last.result = hash((last.result, result));
                last.newest = (call, result);
            }
            _ => {
                if self.ring.len() == WINDOW {
                    self.ring.pop_front();
                }
                self.ring.push_back(Entry {
                    call,
                    result,
                    step,
                    read,
                    writes: self.writes,
                    newest: (call, result),
                });
            }
        }
        if !read {
            self.writes += 1;
        }
    }

    /// The run of calls the newest ones repeat most often, and how many rounds of it
    /// there are; None below two rounds. The shortest run wins a tie, so `A,A,A,A` is
    /// the same call in a row, not `A,A` twice.
    fn period(&self) -> Option<(Shape, u32)> {
        let n = self.ring.len();
        let mut best: Option<(usize, u32)> = None;
        for k in 1..=MAX_PERIOD.min(n / 2) {
            let mut matched = 0;
            while matched + k < n
                && self.ring[n - 1 - matched].same(&self.ring[n - 1 - matched - k])
            {
                matched += 1;
            }
            let rounds = u32::try_from((matched + k) / k).unwrap_or(u32::MAX);
            if rounds >= 2 && best.is_none_or(|(_, most)| rounds > most) {
                best = Some((k, rounds));
            }
        }
        best.map(|(k, rounds)| {
            let shape = if k == 1 {
                Shape::InARow
            } else {
                Shape::Period(k)
            };
            (shape, rounds)
        })
    }

    /// How many times the newest read gave this same result since a call last could
    /// have changed it, within the window and the newest `kept` steps when results older
    /// are put away; None for a call that may write.
    fn again(&self, kept: Option<u32>) -> Option<(Shape, u32)> {
        let last = self.ring.back().filter(|last| last.read)?;
        let oldest = kept.map_or(0, |kept| last.step.saturating_sub(kept.saturating_sub(1)));
        let count = self
            .ring
            .iter()
            .filter(|entry| {
                entry.read
                    && entry.step >= oldest
                    && entry.writes == last.writes
                    && entry.same(last)
            })
            .count();
        Some((Shape::Again, u32::try_from(count).unwrap_or(u32::MAX)))
    }
}

/// The hash a result is compared by: its ends, each line with ids and times made alike,
/// and its rough size, so two long results that differ only in the middle rarely meet.
pub fn result_hash(content: &str) -> u64 {
    let head = &content[..content.floor_char_boundary(RESULT_BYTES)];
    let tail_start = content.ceil_char_boundary(content.len().saturating_sub(RESULT_BYTES));
    // Windows that overlap leave the tail what the head did not read.
    let tail = &content[tail_start.max(head.len())..];
    let lines: Vec<String> = head
        .lines()
        .chain(tail.lines())
        .map(signature::volatile)
        .collect();
    hash((lines, content.len() / 256))
}

fn note(shape: Shape, count: u32) -> String {
    match shape {
        Shape::InARow => format!(
            "[this exact call has returned this same result {count} times in a row; repeating it \
             will not change anything. Read the result again, try a different command, or ask \
             the user.]"
        ),
        Shape::Period(k) => format!(
            "[your last {k} calls repeat the {k} before them with the same results, {count} \
             rounds so far; another round will not change anything. Read the results again, \
             try a different approach, or ask the user.]"
        ),
        Shape::Again => format!(
            "[this call has returned this same result {count} times in your last {WINDOW} \
             calls, with nothing changed between. Use what it said; if you are waiting on \
             something, wait for it to end instead of asking again.]"
        ),
    }
}

fn replan(shape: Shape, count: u32) -> String {
    let what = match shape {
        Shape::InARow => {
            format!("this exact call has returned this same result {count} times in a row")
        }
        Shape::Period(k) => {
            format!("your last {k} calls have come round {count} times with the same results")
        }
        Shape::Again => {
            format!("this call has returned this same result {count} times with nothing changed")
        }
    };
    format!(
        "[{what}; the approach is not working. Before your next call, write one sentence with \
         a different plan, then follow it.]"
    )
}
