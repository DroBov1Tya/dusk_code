use crate::transport::Execution;

#[derive(Debug, Clone)]
pub struct ExecutionFailed {
    pub execution: Execution,
}
