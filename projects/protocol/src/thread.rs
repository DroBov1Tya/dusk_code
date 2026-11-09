//! What a thread tells its client: every change a client shows, as one event type.

use serde::{Deserialize, Serialize};

/// The arbiter's thread, one for the whole config.
pub const ARBITER_THREAD: &str = "arbiter";

/// The name an agent's thread goes by, made from its ID, so every client shows the same
/// one: a word and two hex digits, such as `Fable-3a`.
pub fn agent_name(thread_id: &str) -> String {
    const NAMES: [&str; 8] = [
        "Aster", "Beryl", "Cinder", "Dapple", "Ember", "Fable", "Glint", "Hollow",
    ];
    let seed = name_seed(thread_id);
    format!(
        "{}-{:02x}",
        NAMES[(seed as usize) % NAMES.len()],
        (seed >> 24) & 0xff
    )
}

pub fn name_seed(thread_id: &str) -> u64 {
    fnv1a(thread_id.as_bytes())
}

/// FNV-1a: stable across runs and builds, unlike the standard hasher. Not a defence
/// against anyone.
pub fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325_u64, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
    })
}

use crate::{
    agent::AgentPhase,
    conversation::{Compaction, LocalUnsent, Message},
    goal::Goal,
    model::{Effort, ModelEvent, ToolCall},
    tools::{ApprovalRequest, Question, ToolResult},
    work::{Task, ThreadTokens, Turn},
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ThreadEvent {
    /// The active model changed or was discovered after the thread opened.
    ModelSelected {
        name: String,
        /// The largest request to it: its window or the size set under models.
        input_limit: u64,
    },
    /// Saved locally, outside the model queue, until the client explicitly resends it.
    LocalUnsent(LocalUnsent),
    /// The selected provider could not answer, or no model is selected.
    ProviderUnavailable,
    Queued {
        message_id: String,
    },
    /// A client's request joined the persisted model history, including explicit resends.
    UserMessage(Message),
    Rejected {
        reason: String,
    },
    TurnStarted(Turn),
    /// A model call failed in a way that may pass; the next attempt follows after a pause.
    Retrying {
        attempt: u32,
        error: String,
    },
    /// Everything the model streams: reasoning, text, tool calls, usage.
    Model(ModelEvent),
    ToolStarted(ToolCall),
    ToolFinished {
        call_id: String,
        result: ToolResult,
    },
    TurnFinished(Turn),
    /// The thread got a name: the start of its first message at once, then one the model
    /// gives after the first turn.
    TitleChanged {
        title: String,
    },
    /// The model reached its output cap in one call; the turn ends with what it wrote.
    OutputLimitReached,
    /// The model refused pictures; they stay files it is told the path of.
    PicturesRefused {
        model: String,
    },
    /// The model's answer broke off, such as on a dropped connection; the turn ends with
    /// what it wrote.
    AnswerCut {
        error: String,
    },
    /// The model is summarizing the context; its text streams as `Model` events.
    CompactionStarted,
    CompactionFinished(Compaction),
    /// The context stays as it was.
    CompactionFailed {
        error: String,
    },
    /// The thread's token counts, when it starts and after each turn and compaction. `limit` is the largest request: the
    /// configured input limit or the model window, whichever is smaller.
    Tokens {
        tokens: ThreadTokens,
        limit: u64,
    },
    /// The output generation rate of this thread's agent, over a short rolling window:
    /// about once a second while it generates, and once when a model call ends. A
    /// subagent's rate reaches the client wrapped in `Subagent`, whose `thread_id` says
    /// which agent it is.
    OutputRate {
        /// Generated tokens per second over `window_ms`, counted as context sizes are.
        tokens_per_sec: u64,
        /// How wide the window the average covers is, in milliseconds.
        window_ms: u64,
    },
    /// The context is past the input limit, so the queued turn was not started or the
    /// running one ended; the client should switch model or compact.
    ContextTooLarge {
        tokens: u64,
        limit: u64,
    },
    /// The tool call `call_id` gave `task` to `agent`, which works in thread `thread_id`
    /// and reasons at `effort`; `None` is the provider's default.
    SubagentStarted {
        call_id: String,
        thread_id: String,
        agent: String,
        task: String,
        effort: Option<Effort>,
    },
    /// An event of a subagent's thread. A subagent's own subagents come wrapped once more.
    Subagent {
        thread_id: String,
        event: Box<ThreadEvent>,
    },
    /// The thread's task board changed: every task with its steps.
    Tasks(Vec<Task>),
    /// An agent's phase changed, by its own `agent_state` call or the runtime's.
    AgentPhase {
        phase: AgentPhase,
        note: String,
    },
    /// The agent asks the user to choose; the answer comes as the user's next message.
    Question(Question),
    /// A tool waits for the user's yes; the client answers through the runtime.
    Approval(ApprovalRequest),
    /// The arbiter answered a request of this thread in the user's place.
    ArbiterDecided {
        what: String,
        allow: bool,
        reason: String,
    },
    /// The request is answered or its call is gone; the client stops showing it.
    ApprovalClosed {
        id: u64,
    },
    /// Another agent wrote to this thread; the message has joined it.
    FromAgent(Message),
    /// A background process result joined this thread at a safe execution boundary.
    FromJob(Message),
    /// The standing goal changed, or the thread opened with one; `note` says what
    /// happened in a line.
    Goal {
        goal: Option<Goal>,
        note: String,
        /// The message queued to start work on it, for a client to show as sent.
        started: Option<String>,
    },
    /// /undo took the thread back; `text` is the message it went back to, for the
    /// client to offer again.
    Undone {
        text: String,
    },
    /// /fork wrote its summary; a client opens a new thread that starts from it.
    Forked {
        summary: String,
    },
    /// Nothing is running and nothing will start until the next message.
    Idle,
    /// The thread stopped on an error it could not handle, such as a database failure.
    Failed(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::{ApprovalRequest, Request};

    #[test]
    fn events_survive_a_round_trip() {
        let event = ThreadEvent::Subagent {
            thread_id: "child".into(),
            event: Box::new(ThreadEvent::Approval(ApprovalRequest {
                id: 7,
                request: Request::Command {
                    command: "cargo test".into(),
                },
                session: Some("cargo".into()),
            })),
        };
        let text = serde_json::to_string(&event).unwrap();
        assert_eq!(serde_json::from_str::<ThreadEvent>(&text).unwrap(), event);
    }
}

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
