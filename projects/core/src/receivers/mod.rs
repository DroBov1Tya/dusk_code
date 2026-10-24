//! Method-based entry points. A future public adapter receives only `UserReceiver`.
//! The trusted agent runtime owns `AgentReceiver`; no provider or network server is invoked here.

mod agent;
mod user;

pub use agent::{AgentOutcome, AgentReceipt, AgentReceiver, AgentResponse, AgentTask};
pub use user::{ScenarioRequest, UserMessage, UserReceipt, UserReceiver, UserRequest};
