//! A thread as the list of chats shows it.

use serde::{Deserialize, Serialize};

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Thread {
    pub id: String,
    /// Who started the thread, such as `cli`.
    pub source: String,
    /// Unix timestamp in milliseconds.
    pub created_at: i64,
    /// `None` until the first message.
    pub title: Option<String>,
    /// When the newest message or local unsent input was stored, or `created_at`.
    pub active_at: i64,
    /// For a subagent's thread: the thread that asked it, and the agent's name.
    pub parent_thread_id: Option<String>,
    pub agent: Option<String>,
    /// What an agent's thread goes by, such as `Fable-3a`.
    pub name: Option<String>,
    /// The folder a chat was started in; `None` for one older than this record.
    pub project: Option<String>,
    /// It is kept in memory only and is gone when the core stops.
    #[serde(default)]
    pub temporary: bool,
    /// The id of the token that started the chat; `None` for the local clients' chats. It is
    /// the token id for now; Phase 3 maps it to the name before any non-full scope sees
    /// other chats.
    #[serde(default)]
    pub owner: Option<String>,
    /// An answer, a question, a request for a yes or the end of a turn arrived since a
    /// client last showed the chat (`ThreadRequest::Seen`). Never for an agent's thread.
    #[serde(default)]
    pub unread: bool,
    /// The pull requests the chat linked, the first linked first; merged and closed ones
    /// stay. `ThreadEvent::Pulls` tells of a change. Empty for an agent's thread.
    #[serde(default)]
    pub pulls: Vec<crate::project::ThreadPull>,
}

/// A message found by searching the text of every chat.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Found {
    pub thread_id: String,
    pub title: Option<String>,
    /// The message's place in its chat.
    pub sequence: u64,
    /// The words around the match, on one line.
    pub snippet: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::{Request, ThreadRequest};
    use crate::thread::ThreadEvent;

    /// The unread mark, the request that clears it and the event that tells of it cross
    /// the wire; a thread from a core before the mark reads as read.
    #[test]
    fn the_unread_mark_survives_a_round_trip_and_defaults_to_read() {
        let thread = Thread {
            id: "chat".into(),
            source: "gui".into(),
            created_at: 1,
            title: Some("t".into()),
            active_at: 2,
            parent_thread_id: None,
            agent: None,
            name: None,
            project: None,
            temporary: false,
            owner: None,
            unread: true,
            pulls: Vec::new(),
        };
        let text = serde_json::to_string(&thread).unwrap();
        assert_eq!(serde_json::from_str::<Thread>(&text).unwrap(), thread);
        let mut old = serde_json::to_value(&thread).unwrap();
        old.as_object_mut().unwrap().remove("unread");
        assert!(!serde_json::from_value::<Thread>(old).unwrap().unread);

        let seen = Request::Thread(ThreadRequest::Seen {
            thread_id: "chat".into(),
        });
        let text = serde_json::to_string(&seen).unwrap();
        assert_eq!(serde_json::from_str::<Request>(&text).unwrap(), seen);
        for event in [ThreadEvent::Unread(true), ThreadEvent::Unread(false)] {
            let text = serde_json::to_string(&event).unwrap();
            assert_eq!(serde_json::from_str::<ThreadEvent>(&text).unwrap(), event);
        }
    }
}
