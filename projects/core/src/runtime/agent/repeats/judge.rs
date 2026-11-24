//! What a judge model reads of a stall candidate and how its answer is taken. The rules
//! are cheap and sure of what they count, not of what it means: steps with nothing new
//! may be a long search, and a cycle may be a poll. So a candidate goes to a model with
//! a short digest of the turn (the user's request, the newest calls one line each, what
//! the rule saw, the error that came back most), and its answer is a JSON object
//! `{stuck, confidence, redirect}`. Only a confident "stuck" ends the turn; anything else,
//! an answer that cannot be read included, lets it go on.
//!
//! The digest holds what tools wrote and the model asked, so the judge is told it is
//! data; its answer is read as data too: a redirect goes to the model as one plain line.

use dusk_protocol::text::clip_chars;
use serde_json::Value;

use super::{Digest, Stall};
use crate::plugins::contract::untrusted;

/// Calls a digest lists, the window the cycle rule looks over.
pub const RECENT_CALLS: usize = super::cycles::WINDOW;
/// How sure the judge must be that the turn is stuck to end it.
pub const SURE: f64 = 0.9;
/// Characters of the user's request in a digest.
const MAX_REQUEST_CHARS: usize = 1500;
/// Characters of the error in a digest.
const MAX_ERROR_CHARS: usize = 1200;
/// Characters of the judge's redirect the model reads.
const MAX_REDIRECT_CHARS: usize = 300;
/// Calls a stopped turn lists as what was tried.
const TRIED_CALLS: usize = 8;

/// The judge's answer.
#[derive(Debug, Clone, PartialEq)]
pub struct Verdict {
    pub stuck: bool,
    /// From 0 to 1.
    pub confidence: f64,
    /// What the agent should do instead, one plain line; may be empty.
    pub redirect: String,
}

impl Verdict {
    /// Whether the turn ends.
    pub fn sure_stuck(&self) -> bool {
        self.stuck && self.confidence >= SURE
    }
}

/// The message the judge reads.
pub fn digest(request: &str, stall: &Stall, digest: &Digest) -> String {
    let request = clip_chars(request.trim(), MAX_REQUEST_CHARS, "…");
    let calls = match digest.calls.is_empty() {
        true => "(none kept)".to_owned(),
        false => digest.calls.join("\n"),
    };
    let mut text = format!(
        "What the rule saw: {}.\n\nThe user's request:\n<<<\n{request}\n>>>\n\nThe newest \
         calls of the turn, oldest first:\n<<<\n{calls}\n>>>",
        stall.what
    );
    if let Some(error) = &digest.error {
        let error = clip_chars(error, MAX_ERROR_CHARS, "…");
        text.push_str(&format!(
            "\n\nThe error that came back most:\n<<<\n{error}\n>>>"
        ));
    }
    text
}

/// The judge's answer read from its text: the first JSON object in it. None when there
/// is none or it says nothing of `stuck`; a confidence out of range is brought into it.
pub fn verdict(text: &str) -> Option<Verdict> {
    let start = text.find('{')?;
    let end = text.rfind('}')?;
    let object: Value = serde_json::from_str(text.get(start..=end)?).ok()?;
    let stuck = match &object["stuck"] {
        Value::Bool(stuck) => *stuck,
        Value::String(stuck) => match stuck.trim().to_ascii_lowercase().as_str() {
            "true" | "yes" => true,
            "false" | "no" => false,
            _ => return None,
        },
        _ => return None,
    };
    let confidence = match &object["confidence"] {
        Value::Number(number) => number.as_f64().unwrap_or(0.0),
        Value::String(number) => number.trim().parse().unwrap_or(0.0),
        _ => 0.0,
    };
    let confidence = match confidence.is_finite() {
        true => confidence.clamp(0.0, 1.0),
        false => 0.0,
    };
    let redirect = object["redirect"].as_str().unwrap_or_default();
    let redirect = untrusted::line(redirect.trim(), MAX_REDIRECT_CHARS);
    let redirect = redirect.split_whitespace().collect::<Vec<_>>().join(" ");
    Some(Verdict {
        stuck,
        confidence,
        redirect,
    })
}

/// What the model reads after a judge let the turn go on with advice.
pub fn judge_note(stall: &Stall, redirect: &str) -> String {
    format!(
        "[a reviewer read your recent steps ({}) and suggests: {redirect} Weigh it, then \
         go on.]",
        stall.what
    )
}

/// Why the turn ended, for the user or the asking agent: what the rule saw, what was
/// tried, and what the judge would do instead.
pub fn judge_stop(stall: &Stall, verdict: &Verdict, digest: &Digest) -> String {
    let skip = digest.calls.len().saturating_sub(TRIED_CALLS);
    let tried: Vec<String> = digest
        .calls
        .iter()
        .skip(skip)
        .map(|call| format!("- {call}"))
        .collect();
    let mut why = format!(
        "the agent stopped: {}, and a reviewer found the turn stuck.",
        stall.what
    );
    if !tried.is_empty() {
        why.push_str(&format!("\nlast calls:\n{}", tried.join("\n")));
    }
    if !verdict.redirect.is_empty() {
        why.push_str(&format!("\nsuggested: {}", verdict.redirect));
    }
    why
}
