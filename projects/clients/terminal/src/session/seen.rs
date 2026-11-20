//! Telling the core the chat of the shown tab was read: once it is shown, and again each
//! time the core says something unread arrived in it.

use dusk_core::client::Client;
use dusk_protocol::{client::Event, thread::ThreadEvent};

/// The chat last marked read, and whether it turned unread since.
#[derive(Default)]
pub struct Seen {
    marked: Option<String>,
    again: bool,
}

impl Seen {
    /// Hears what the core sent: an unread mark in the chat on screen asks for another.
    pub fn hear(&mut self, event: &Event) {
        if let Event::Thread { thread_id, event } = event
            && matches!(**event, ThreadEvent::Unread(true))
            && self.marked.as_deref() == Some(thread_id.as_str())
        {
            self.again = true;
        }
    }

    /// The chat to mark read now: the shown one, when it is new on screen or turned
    /// unread while there.
    fn due(&mut self, shown: Option<&str>) -> Option<String> {
        if self.marked.as_deref() == shown && !std::mem::take(&mut self.again) {
            return None;
        }
        self.again = false;
        self.marked = shown.map(str::to_owned);
        self.marked.clone()
    }

    /// Marks the shown chat read when it is due. A core that cannot is no reason to stop
    /// the screen: the mark stays lit.
    pub async fn mark(&mut self, client: &Client, shown: Option<&str>) {
        if let Some(thread_id) = self.due(shown)
            && let Err(error) = client.threads().seen(&thread_id).await
        {
            tracing::warn!("cannot mark the chat read: {error:#}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unread(thread_id: &str) -> Event {
        Event::Thread {
            thread_id: thread_id.into(),
            event: Box::new(ThreadEvent::Unread(true)),
        }
    }

    #[test]
    fn a_chat_is_marked_once_shown_and_again_when_it_turns_unread_on_screen() {
        let mut seen = Seen::default();
        assert_eq!(seen.due(None), None);
        assert_eq!(seen.due(Some("a")).as_deref(), Some("a"));
        assert_eq!(seen.due(Some("a")), None, "once");
        seen.hear(&unread("b"));
        assert_eq!(seen.due(Some("a")), None, "another chat's mark");
        seen.hear(&unread("a"));
        assert_eq!(seen.due(Some("a")).as_deref(), Some("a"));
        assert_eq!(seen.due(Some("b")).as_deref(), Some("b"), "another tab");
        assert_eq!(seen.due(None), None);
        assert_eq!(seen.due(Some("b")).as_deref(), Some("b"), "shown again");
    }
}
