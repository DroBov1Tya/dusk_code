//! Entry points into the core: clients get `UserReceiver`, the agent runtime `AgentReceiver`.

mod agent_receiver;
mod user_receiver;

pub use agent_receiver::{AgentOutcome, AgentReceiver, AgentStep, Conversation};
pub use user_receiver::{ScenarioRequest, UserMessage, UserReceipt, UserReceiver, UserRequest};
