//! Method-based entry points. A future public adapter receives only `UserReceiver`.
//! The trusted agent runtime owns `AgentReceiver`; no provider or network server is invoked here.

mod agent_receiver;
mod user_receiver;

pub use agent_receiver::{AgentOutcome, AgentReceipt, AgentReceiver, AgentResponse, AgentTask};
pub use user_receiver::{ScenarioRequest, UserMessage, UserReceipt, UserReceiver, UserRequest};
