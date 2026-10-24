use crate::transport::Execution;

#[derive(Debug, Clone)]
pub struct TaskRejected {
    pub execution: Execution,
}
