/// Only receipt is implemented; accepting a task does not complete execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionStatus {
    Accepted,
    Rejected,
}

impl ExecutionStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Accepted => "accepted",
            Self::Rejected => "rejected",
        }
    }
}

/// Fixed validation codes never contain user-message content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RejectionCode {
    EmptyMessage,
    EmptyThread,
    UnknownThread,
}

impl RejectionCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::EmptyMessage => "empty_message",
            Self::EmptyThread => "empty_thread",
            Self::UnknownThread => "unknown_thread",
        }
    }
}

/// Durable receipt without the original message or generated output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Execution {
    pub request_uid: String,
    pub thread_id: String,
    /// Unix timestamp in milliseconds, assigned on first receipt.
    pub received_at: i64,
    pub status: ExecutionStatus,
    pub error_code: Option<RejectionCode>,
}

impl Execution {
    /// Rejection is terminal; accepted requests await execution and remain incomplete.
    pub fn is_complete(&self) -> bool {
        match self.status {
            ExecutionStatus::Accepted => false,
            ExecutionStatus::Rejected => true,
        }
    }
}
