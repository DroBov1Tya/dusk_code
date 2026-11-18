//! What a chat shows of its steps, shared by the clients that fold a thread into a list:
//! a tool step as plain data, what a subagent does now, the line each event that is not a
//! message leaves in the chat, the one row of a model call being tried again, what the
//! running turn has used, and the coloured labels inside a message's text. The clients
//! keep the list and its layout.

mod agent;
mod mark;
mod note;
mod progress;
mod retry;
mod tool;

pub use agent::AgentState;
pub use mark::{MAX_MARK_CHARS, Span, Tone, marks, unmarked};
pub use note::{Note, note};
pub use progress::{TurnSpend, turn_spend};
pub use retry::{Retry, RetryRow, retry_row};
pub use tool::{DETAIL_KEYS, SHOWN_CHARS, Tool, ToolState, clip, tool_detail};

#[cfg(test)]
mod tests;
