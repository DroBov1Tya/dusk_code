//! The failure ladder: the same error coming back within a turn, through calls that
//! differ, gets help that grows with the count. Two is normal; then a note to stop
//! varying the call, then a request for a different plan, then the turn stops. Counts are
//! not only in a row: a stuck model often reads a file between two failing commands.
//!
//! The count is kept here for two readers: the ladder's notes (`climb`) and the episodes
//! a pass after a counted failure makes (`solved`).

use std::collections::HashMap;

use dusk_protocol::thread::RepeatRung;

use super::{
    hash,
    signature::{self, Signature},
};
use crate::{config::Limits, project::episodes, providers::ToolCall};

/// Distinct failures a turn keeps count of; more are not counted, so a turn that
/// fails in many ways cannot grow this without bound.
const MAX_SIGNATURES: usize = 64;
/// Distinct attempts kept per failure, to list them back.
const MAX_ATTEMPTS: usize = 6;
/// An attempt is listed on one line at most this long.
const MAX_ATTEMPT_CHARS: usize = 100;

#[derive(Default)]
pub struct Ladder {
    pub(super) seen: HashMap<u64, Seen>,
    /// Failures counted in this turn, so the one that failed last is known.
    pub(super) failures: u64,
    /// Episodes this turn found, against their cap.
    pub(super) solved: u32,
}

#[derive(Default)]
pub(super) struct Seen {
    count: u32,
    pub(super) attempts: Vec<Attempt>,
    /// More distinct attempts came than are kept.
    more: bool,
    /// The signature's key and its latest error lines, cut and whole.
    pub(super) key: String,
    pub(super) excerpt: String,
    pub(super) error: String,
    /// Failures since a pass last ended them: what a pass now would end.
    pub(super) open: u32,
    /// Unix ms of the first of the open failures.
    pub(super) opened_at: i64,
    /// Which failure of the turn this signature's latest was.
    pub(super) last: u64,
    /// The program and first argument of its failing commands, each once.
    pub(super) invocations: Vec<(String, Option<String>)>,
}

/// What the ladder does about one failed call.
pub struct Climb {
    pub rung: RepeatRung,
    pub count: u32,
    /// Added to the result the model reads.
    pub note: String,
    /// At the top: why the turn ends, for the user or the asking agent.
    pub stop: Option<String>,
}

impl Ladder {
    /// Counts a failed call under its signature; the signature's hash, or None when the
    /// output has nothing to recognize or the turn keeps count of too many.
    pub fn count(&mut self, call: &ToolCall, output: &str) -> Option<u64> {
        let signature = signature::of(&call.name, &call.arguments, output)?;
        self.count_signature(call, signature)
    }

    /// Counts a failed call under a signature found another way, as a test run's
    /// failing ids.
    pub fn count_signature(&mut self, call: &ToolCall, signature: Signature) -> Option<u64> {
        let key = hash(&signature.key);
        if !self.seen.contains_key(&key) && self.seen.len() >= MAX_SIGNATURES {
            return None;
        }
        self.failures += 1;
        let seen = self.seen.entry(key).or_default();
        seen.count = seen.count.saturating_add(1);
        seen.add(Attempt::of(call));
        if seen.open == 0 {
            seen.opened_at = dusk_protocol::system::now_ms();
        }
        seen.open = seen.open.saturating_add(1);
        seen.last = self.failures;
        if let Some(invocation) = invocation(call)
            && !seen.invocations.contains(&invocation)
            && seen.invocations.len() < MAX_ATTEMPTS
        {
            seen.invocations.push(invocation);
        }
        seen.key = signature.key;
        seen.excerpt = signature.excerpt;
        seen.error = signature.error;
        Some(key)
    }

    /// What to do once the error counted under `key` has come back often enough. The
    /// turn stops at `stop_at`; with None, the top rung asks for a different plan again
    /// instead.
    pub fn climb(&self, key: u64, limits: &Limits, stop_at: Option<u32>) -> Option<Climb> {
        let seen = self.seen.get(&key)?;
        let count = seen.count;
        let attempts = seen.list();
        let (rung, note, stop) = if stop_at.is_some_and(|at| count >= at) {
            let note = format!(
                "[this same error has come back {count} times in this turn; the turn stops here.]"
            );
            let stop = format!(
                "the agent stopped: the same error came back {count} times in this turn.\n\
                 error:\n{}\nattempts:\n{attempts}",
                seen.excerpt
            );
            (RepeatRung::Stop, note, Some(stop))
        } else if count >= limits.failure_replan_at {
            let then = stop_at
                .map(|at| format!(" At {at} the turn stops."))
                .unwrap_or_default();
            let note = format!(
                "[this same error has come back {count} times in this turn; the approach is \
                 not working. Attempts so far:\n{attempts}\nBefore your next call, write one \
                 sentence with a different plan, then follow it.{then}]"
            );
            (RepeatRung::Replan, note, None)
        } else if count >= limits.failure_note_at {
            let note = format!(
                "[this same error has come back {count} times in this turn. Attempts so \
                 far:\n{attempts}\nStop varying the command. Read the error itself, check the \
                 assumption behind it (does the file, tool or dependency exist?), or take a \
                 different approach.]"
            );
            (RepeatRung::SameError, note, None)
        } else {
            return None;
        };
        Some(Climb {
            rung,
            count,
            note,
            stop,
        })
    }
}

impl Ladder {
    /// The error that came back most often in the turn, with its attempts; None when
    /// none came back.
    pub fn worst(&self) -> Option<String> {
        let seen = self
            .seen
            .values()
            .filter(|seen| seen.count >= 2)
            .max_by_key(|seen| (seen.count, seen.last))?;
        Some(format!(
            "this error came back {} times:\n{}\nattempts:\n{}",
            seen.count,
            seen.excerpt,
            seen.list()
        ))
    }
}

impl Seen {
    /// Attempts are told apart as they are listed, so the list reads as it always did.
    fn add(&mut self, attempt: Attempt) {
        if self.attempts.iter().any(|kept| kept.shown == attempt.shown) {
            return;
        }
        if self.attempts.len() < MAX_ATTEMPTS {
            self.attempts.push(attempt);
        } else {
            self.more = true;
        }
    }

    fn list(&self) -> String {
        let mut lines: Vec<String> = self
            .attempts
            .iter()
            .map(|attempt| format!("- {}", attempt.shown))
            .collect();
        if self.more {
            lines.push("- and others".to_owned());
        }
        lines.join("\n")
    }
}

/// The program and first argument of a command call; None for any other tool.
pub(super) fn invocation(call: &ToolCall) -> Option<(String, Option<String>)> {
    if call.name != signature::RUN_COMMAND {
        return None;
    }
    signature::invocation(call.arguments["command"].as_str()?)
}

/// A call on one line: a command as written, another tool with its arguments.
pub(super) struct Attempt {
    /// Cut short, as the ladder lists it to the model.
    pub(super) shown: String,
    /// Whole, as far as an episode reads (`episodes::bounded`): an episode cleans it
    /// before it cuts it, so no half of a key escapes the rules.
    pub(super) whole: String,
}

impl Attempt {
    pub(super) fn of(call: &ToolCall) -> Self {
        let text = match call.arguments["command"].as_str() {
            Some(command) if call.name == signature::RUN_COMMAND => command.to_owned(),
            _ => format!("{} {}", call.name, call.arguments),
        };
        let words: Vec<&str> = episodes::bounded(&text).split_whitespace().collect();
        Self {
            shown: shown(&text),
            whole: words.join(" "),
        }
    }
}

/// `text` on one line of `MAX_ATTEMPT_CHARS` at most.
fn shown(text: &str) -> String {
    let mut line = String::new();
    for word in text.split_whitespace() {
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
        if line.chars().count() > MAX_ATTEMPT_CHARS {
            break;
        }
    }
    match line.char_indices().nth(MAX_ATTEMPT_CHARS) {
        Some((end, _)) => format!("{}…", &line[..end]),
        None => line,
    }
}
