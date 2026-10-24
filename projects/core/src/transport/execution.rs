/// Accepting a task does not complete execution; rejection and failure are terminal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionStatus {
    Accepted,
    Rejected,
    Failed,
}

impl ExecutionStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Accepted => "accepted",
            Self::Rejected => "rejected",
            Self::Failed => "failed",
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

/// Fixed execution-failure codes never contain provider errors or message content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionFailureCode {
    ExecutionFailed,
}

impl ExecutionFailureCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ExecutionFailed => "execution_failed",
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
    /// Validation rejection; absent for accepted and failed executions.
    pub error_code: Option<RejectionCode>,
    /// Execution failure; distinct from validation rejection.
    pub failure_code: Option<ExecutionFailureCode>,
}

impl Execution {
    /// Rejection and failure are terminal; accepted requests remain incomplete.
    pub fn is_complete(&self) -> bool {
        match self.status {
            ExecutionStatus::Accepted => false,
            ExecutionStatus::Rejected | ExecutionStatus::Failed => true,
        }
    }
}
