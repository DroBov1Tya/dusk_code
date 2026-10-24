use crate::transport::Execution;

#[derive(Debug, Clone)]
pub struct TaskAccepted {
    pub execution: Execution,
}
