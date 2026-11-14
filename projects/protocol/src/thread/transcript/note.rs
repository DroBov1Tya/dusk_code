use crate::thread::ThreadEvent;

/// A line an event leaves in the chat.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Note {
    Notice(String),
    Error(String),
}

/// The line an event that is not a message or a step leaves in the chat, if any.
pub fn note(event: &ThreadEvent) -> Option<Note> {
    use Note::{Error, Notice};
    Some(match event {
        ThreadEvent::Failed(error) => Error(error.clone()),
        ThreadEvent::Rejected { reason } => Error(reason.clone()),
        ThreadEvent::ProviderUnavailable => {
            Error("The provider is unavailable; check the connection in the settings.".into())
        }
        ThreadEvent::Retrying { attempt, error } => {
            Notice(format!("Retrying ({attempt}): {error}"))
        }
        ThreadEvent::AnswerCut { error } => Error(error.clone()),
        ThreadEvent::CompactionFailed { error } => Error(format!("Compaction failed: {error}")),
        ThreadEvent::OutputLimitReached => Notice("The answer reached the output limit.".into()),
        ThreadEvent::PicturesRefused { model } => {
            Notice(format!("{model} does not take pictures."))
        }
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
        ThreadEvent::Goal { note, .. } => Notice(note.clone()),
        ThreadEvent::Forked { .. } => Notice("Forked into a new chat.".into()),
        _ => return None,
    })
}
