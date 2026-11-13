//! A thread as the list of chats shows it.

use serde::{Deserialize, Serialize};

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
}

/// A message found by searching the text of every chat.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Found {
    pub thread_id: String,
    pub title: Option<String>,
    /// The message's place in its chat.
    pub sequence: u64,
    /// The words around the match, on one line.
    pub snippet: String,
}
