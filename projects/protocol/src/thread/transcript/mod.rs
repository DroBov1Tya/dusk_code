//! What a chat shows, shared by every client: the chat itself as `ChatLog`, folded from the
//! stored history and the live events, and its pieces: a tool step as plain data, what a
//! subagent does now, the line each event that is not a message leaves in the chat, the
//! one row of a model call being tried again, what the running turn has used, and the
//! coloured labels inside a message's text. The clients keep only the drawing.

mod agent;
mod log;
mod mark;
mod note;
mod progress;
mod retry;
mod run;
mod tool;

pub use agent::AgentState;
pub use log::{Applied, CHAT_ASKS, ChatLog, INTERRUPTED, Item, Subagent, UNFINISHED, context_line};
pub use mark::{MAX_MARK_CHARS, Span, Tone, marks, unmarked};
pub use note::{Note, note};
pub use progress::{TurnSpend, turn_spend};
pub use retry::{Retry, RetryRow, retry_row};
pub use run::{ASK_AGENT_TOOL, PLAN_TOOL, Run, RunCard, RunKind, RunMark};
pub use tool::{
    DETAIL_KEYS, FAILED_PREFIXES, SHOWN_CHARS, Tool, ToolState, clip, stored_failure, tool_detail,
};

#[cfg(test)]
mod tests;
