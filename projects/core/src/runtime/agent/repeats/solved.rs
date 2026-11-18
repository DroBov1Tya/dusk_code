//! A loop that ended: the same error came back, and then a call like the ones that failed
//! passed. What the loop was becomes a `fixed` episode, and the model may be asked for its
//! lesson.
//!
//! A call passed when it is `run_command` that ran in the foreground and exited 0: its
//! result is no error and its exit code is 0. A background start, a command cut off, and
//! any other tool say nothing of the loop, so they end none.

use super::ladder::{Attempt, Ladder, invocation};
use crate::{providers::ToolCall, tools::ToolResult};

/// Failures of one signature before a pass that make a loop; one is ordinary work.
const MIN_FAILURES: u32 = 2;
/// Loops a turn makes episodes of, at most.
const MAX_A_TURN: u32 = 3;
/// What the model's lesson line starts with.
const LESSON: &str = "Lesson:";

/// A loop the call at hand ended.
#[derive(Debug, PartialEq, Eq)]
pub struct Solved {
    /// The signature's key: what makes two such errors the same one.
    pub key: String,
    /// The error lines, as the tool wrote them, whole.
    pub problem: String,
    /// The calls that failed, oldest first, whole.
    pub attempts: Vec<String>,
    /// The call that passed, on one line as an attempt is, whole.
    pub passed: String,
    /// How many times the error came back before the pass.
    pub count: u32,
    /// Unix ms of the loop's first failure: what the ledger recorded since led to the pass.
    pub since: i64,
}

impl Ladder {
    /// The loop `call` ended, if it passed. Of the errors its command failed with since the
    /// last pass, only the one that failed last makes an episode, when it came back; the
    /// others are ended as well, so a later pass does not make them one.
    pub fn solved(&mut self, call: &ToolCall, result: &ToolResult) -> Option<Solved> {
        if result.is_error || result.exit_code != Some(0) {
            return None;
        }
        let ran = invocation(call)?;
        let mut last = None;
        for seen in self.seen.values_mut() {
            if seen.open == 0 || !seen.invocations.contains(&ran) {
                continue;
            }
            let open = std::mem::take(&mut seen.open);
            if last.as_ref().is_some_and(|(at, _)| *at > seen.last) {
                continue;
            }
            let solved = (open >= MIN_FAILURES).then(|| Solved {
                key: seen.key.clone(),
                problem: seen.error.clone(),
                attempts: seen
                    .attempts
                    .iter()
                    .map(|attempt| attempt.whole.clone())
                    .collect(),
                passed: Attempt::of(call).whole,
                count: open,
                since: seen.opened_at,
            });
            last = Some((seen.last, solved));
        }
        let solved = last.and_then(|(_, solved)| solved)?;
        if self.solved >= MAX_A_TURN {
            return None;
        }
        self.solved += 1;
        Some(solved)
    }
}

/// Added to the result of a call that ended a loop, so the answer carries the lesson.
pub fn lesson_note(count: u32) -> String {
    format!(
        "[this passed after the same error came back {count} times. End your answer with \
         one line: {LESSON} <what was wrong, under 20 words>]"
    )
}

/// The model's lesson: the text of the last line of `answer` that starts with
/// `Lesson:`; None when there is no such line or it says nothing.
pub fn lesson(answer: &str) -> Option<&str> {
    answer
        .lines()
        .rev()
        .find_map(|line| line.trim().strip_prefix(LESSON))
        .map(str::trim)
        .filter(|lesson| !lesson.is_empty())
}
