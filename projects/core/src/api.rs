//! Core API identity and request contracts shared by the host and extensions.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ApiVersion {
    pub major: u16,
    pub minor: u16,
}

pub const CORE_API_VERSION: ApiVersion = ApiVersion { major: 0, minor: 1 };

pub use crate::receivers::{
    AgentOutcome, AgentReceipt, AgentReceiver, AgentResponse, AgentTask, ScenarioRequest,
    UserMessage, UserReceipt, UserReceiver, UserRequest,
};
pub use crate::state::{
    Job, JobInput, JobOutcome, JobStatus, Message, MessageAttachment, MessageInput, MessageRole,
    Turn, TurnStatus,
};
pub use crate::transport::{Execution, ExecutionFailureCode, ExecutionStatus, RejectionCode, Task};

// Compatibility negotiation belongs here when extension loading is implemented.
