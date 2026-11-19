//! The user's likely next message, as the model proposes it with `suggest_next`: a
//! composer shows it greyed in its empty input, and Tab takes it in. The core keeps the
//! newest one of a chat and sends it as `ThreadEvent::NextSuggested`; every client keeps
//! it the same way, through `Suggestion`.

use schemars::JsonSchema;
use serde::Deserialize;

use super::ThreadEvent;

/// The tool that proposes the user's next message.
pub const SUGGEST_NEXT_TOOL: &str = "suggest_next";
/// The longest suggestion, in characters; a longer one is cut.
pub const SUGGESTION_CHARS: usize = 200;

/// The arguments of `suggest_next`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SuggestNext {
    /// The message as the user would write it, in their language: one short line.
    pub text: String,
}

impl ThreadEvent {
    /// The event makes the suggestion it follows stale: the user said something, a new
    /// turn began, or the chat went back or forked.
    pub fn ends_suggestion(&self) -> bool {
        matches!(
            self,
            Self::Queued { .. }
                | Self::UserMessage(_)
                | Self::LocalUnsent(_)
                | Self::TurnStarted(_)
                | Self::Undone { .. }
                | Self::Forked { .. }
        )
    }
}

/// A composer's suggestion: the newest one the chat heard, shown while the input is
/// empty and the user has not dismissed it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Suggestion {
    text: Option<String>,
    dismissed: bool,
}

impl Suggestion {
    /// Follows the chat's events: a new suggestion replaces the old one, and whatever makes
    /// it stale drops it. Only the chat's own events count; a subagent's come wrapped and
    /// are never the user's.
    pub fn hear(&mut self, event: &ThreadEvent) {
        match event {
            ThreadEvent::NextSuggested { text } => {
                let text = text.trim();
                *self = Self {
                    text: (!text.is_empty()).then(|| text.to_owned()),
                    dismissed: false,
                };
            }
            event if event.ends_suggestion() => self.clear(),
            _ => {}
        }
    }

    /// The text to show greyed after the caret: only while `input` is empty.
    pub fn ghost(&self, input: &str) -> Option<&str> {
        match (&self.text, self.dismissed) {
            (Some(text), false) if input.is_empty() => Some(text),
            _ => None,
        }
    }

    /// What Tab puts in the input, when the ghost shows; `None` leaves Tab its own meaning.
    /// The suggestion stays: emptying the input again shows it again.
    pub fn accept(&self, input: &str) -> Option<String> {
        self.ghost(input).map(str::to_owned)
    }

    /// Esc: hidden until the next suggestion.
    pub fn dismiss(&mut self) -> bool {
        let shown = self.text.is_some() && !self.dismissed;
        self.dismissed = true;
        shown
    }

    /// The user sent a message, or the chat changed.
    pub fn clear(&mut self) {
        *self = Self::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn suggested(text: &str) -> ThreadEvent {
        ThreadEvent::NextSuggested { text: text.into() }
    }

    #[test]
    fn the_event_survives_a_round_trip() {
        let event = suggested("Да, встраивай все");
        let text = serde_json::to_string(&event).unwrap();
        assert_eq!(text, r#"{"NextSuggested":{"text":"Да, встраивай все"}}"#);
        assert_eq!(serde_json::from_str::<ThreadEvent>(&text).unwrap(), event);
    }

    #[test]
    fn the_ghost_shows_only_on_an_empty_input() {
        let mut suggestion = Suggestion::default();
        assert_eq!(suggestion.ghost(""), None);
        suggestion.hear(&suggested(" run the tests "));
        assert_eq!(suggestion.ghost(""), Some("run the tests"));
        // Typing hides it; clearing the input brings it back.
        assert_eq!(suggestion.ghost("r"), None);
        assert_eq!(suggestion.accept("r"), None);
        assert_eq!(suggestion.ghost(""), Some("run the tests"));
        assert_eq!(suggestion.accept("").as_deref(), Some("run the tests"));
        // Accepting keeps it: the input is full now, and emptied it shows again.
        assert_eq!(suggestion.ghost(""), Some("run the tests"));
        suggestion.hear(&suggested("   "));
        assert_eq!(suggestion.ghost(""), None);
    }

    #[test]
    fn esc_hides_it_until_the_next_one() {
        let mut suggestion = Suggestion::default();
        assert!(!suggestion.dismiss());
        suggestion.hear(&suggested("commit it"));
        assert!(suggestion.dismiss());
        assert_eq!(suggestion.ghost(""), None);
        assert_eq!(suggestion.accept(""), None);
        suggestion.hear(&suggested("push it"));
        assert_eq!(suggestion.ghost(""), Some("push it"));
    }

    #[test]
    fn what_the_user_says_or_a_new_turn_drops_it() {
        for event in [
            ThreadEvent::Queued {
                message_id: "m".into(),
            },
            ThreadEvent::Undone { text: "x".into() },
            ThreadEvent::Forked {
                summary: "s".into(),
            },
        ] {
            let mut suggestion = Suggestion::default();
            suggestion.hear(&suggested("yes"));
            suggestion.hear(&event);
            assert_eq!(suggestion.ghost(""), None, "{event:?}");
        }
        let mut suggestion = Suggestion::default();
        suggestion.hear(&suggested("yes"));
        suggestion.hear(&ThreadEvent::Idle);
        suggestion.hear(&ThreadEvent::Subagent {
            thread_id: "child".into(),
            event: Box::new(suggested("no")),
        });
        assert_eq!(suggestion.ghost(""), Some("yes"));
    }
}
