//! Agents talking to agents: `ask_agent` hands work to another agent, `message_parent`
//! writes back while working, and `agent_state` says where in its work an agent is.

mod agent_state;
mod ask_agent;
mod message_parent;

pub use agent_state::AgentState;
pub use ask_agent::AskAgent;
pub use message_parent::MessageParent;
