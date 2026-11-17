//! The failure ladder: the same error coming back within a turn, through calls that
//! differ, gets help that grows with the count. Two is normal; then a note to stop
//! varying the call, then a request for a different plan, then the turn stops. Counts are
//! not only in a row: a stuck model often reads a file between two failing commands.

use std::collections::HashMap;

use dusk_protocol::thread::RepeatRung;

use super::{hash, signature};
use crate::{config::Limits, providers::ToolCall};

/// Distinct failures a turn keeps count of; more are not counted, so a turn that
/// fails in many ways cannot grow this without bound.
const MAX_SIGNATURES: usize = 64;
/// Distinct attempts kept per failure, to list them back.
const MAX_ATTEMPTS: usize = 6;
/// An attempt is listed on one line at most this long.
const MAX_ATTEMPT_CHARS: usize = 100;

#[derive(Default)]
pub struct Ladder {
    seen: HashMap<u64, Seen>,
}

#[derive(Default)]
struct Seen {
    count: u32,
    attempts: Vec<String>,
    /// More distinct attempts came than are kept.
    more: bool,
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
    /// Counts a failed call; what to do once its error has come back often enough.
    pub fn climb(&mut self, call: &ToolCall, output: &str, limits: &Limits) -> Option<Climb> {
        let signature = signature::of(&call.name, &call.arguments, output)?;
        let key = hash(&signature.key);
        if !self.seen.contains_key(&key) && self.seen.len() >= MAX_SIGNATURES {
            return None;
        }
        let seen = self.seen.entry(key).or_default();
        seen.count = seen.count.saturating_add(1);
        seen.add(attempt(call));
        let count = seen.count;
        let attempts = seen.list();
        let stop_at = limits.failure_stop_at;
        let (rung, note, stop) = if count >= stop_at {
            let note = format!(
                "[this same error has come back {count} times in this turn; the turn stops here.]"
            );
            let stop = format!(
                "the agent stopped: the same error came back {count} times in this turn.\n\
                 error:\n{}\nattempts:\n{attempts}",
                signature.excerpt
            );
            (RepeatRung::Stop, note, Some(stop))
        } else if count >= limits.failure_replan_at {
            let note = format!(
                "[this same error has come back {count} times in this turn; the approach is \
                 not working. Attempts so far:\n{attempts}\nBefore your next call, write one \
                 sentence with a different plan, then follow it. At {stop_at} the turn stops.]"
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

impl Seen {
    fn add(&mut self, attempt: String) {
        if self.attempts.contains(&attempt) {
            return;
        }
        if self.attempts.len() < MAX_ATTEMPTS {
            self.attempts.push(attempt);
        } else {
            self.more = true;
        }
    }

    fn list(&self) -> String {
        let mut lines: Vec<String> = self.attempts.iter().map(|a| format!("- {a}")).collect();
        if self.more {
            lines.push("- and others".to_owned());
        }
        lines.join("\n")
    }
}

/// A call on one short line: a command as written, another tool with its arguments.
fn attempt(call: &ToolCall) -> String {
    let text = match call.arguments["command"].as_str() {
        Some(command) if call.name == signature::RUN_COMMAND => command.to_owned(),
        _ => format!("{} {}", call.name, call.arguments),
    };
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
