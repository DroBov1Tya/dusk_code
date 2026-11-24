use super::{RetryRow, retry_row};
use crate::thread::ThreadEvent;

/// A line an event leaves in the chat.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Note {
    Notice(String),
    Error(String),
}

/// The line an event that is not a message or a step leaves in the chat, if any. A model
/// call tried again leaves one line at its first retry; a client that keeps the in-place
/// row of `retry_row` takes `Retrying` before asking here.
pub fn note(event: &ThreadEvent) -> Option<Note> {
    use Note::{Error, Notice};
    Some(match event {
        ThreadEvent::TurnWaiting(turn) => Notice(turn.waiting?.text(None)),
        ThreadEvent::TurnFailed(failure) => Error(failure.text()),
        ThreadEvent::Failed(error) => Error(error.clone()),
        ThreadEvent::Rejected { reason } => Error(reason.clone()),
        ThreadEvent::NoModel => Error("No model is chosen; connect a provider.".into()),
        event @ ThreadEvent::Retrying { attempt: 1, .. } => match retry_row(event) {
            Some(RetryRow::Show(retry)) => Notice(retry.text()),
            _ => return None,
        },
        ThreadEvent::AnswerCut { error } => Error(format!(
            "The answer broke off ({error}); what it wrote is kept."
        )),
        ThreadEvent::CompactionFailed { error } => Error(format!("Compaction failed: {error}")),
        ThreadEvent::OutputLimitReached => Notice("The answer reached the output limit.".into()),
        ThreadEvent::PicturesRefused { model } => Notice(format!(
            "{model} does not take pictures; it gets their paths instead."
        )),
        ThreadEvent::CompactionStarted => Notice("Compacting the context...".into()),
        ThreadEvent::CompactionFinished(_) => Notice("The context was compacted.".into()),
        ThreadEvent::ContextTooLarge { tokens, limit } => Error(format!(
            "The context is too large: {tokens} of {limit} tokens."
        )),
        ThreadEvent::ArbiterDecided {
            what,
            allow,
            reason,
        } => {
            let verdict = if *allow { "allowed" } else { "denied" };
            Notice(format!("Arbiter {verdict} {what}: {reason}"))
        }
        ThreadEvent::Goal { note, .. } if !note.is_empty() => Notice(note.clone()),
        ThreadEvent::Forked { .. } => Notice("Forked into a new chat.".into()),
        _ => return None,
    })
}
