//! Agents talking to agents: `ask_agent` hands work to another agent, `message_parent`
//! writes back while working.

mod ask_agent;
mod message_parent;

pub use ask_agent::AskAgent;
pub use message_parent::MessageParent;
