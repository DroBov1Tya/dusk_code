use super::{
    ExecutionFailed, JobCreated, JobUpdated, LifecycleChanged, MessageCreated, TaskAccepted,
    TaskRejected, ThreadCreated, ThreadUpdated,
};

#[derive(Debug, Clone)]
pub enum Event {
    DatabaseCreated(super::DatabaseCreated),
    ThreadCreated(ThreadCreated),
    ThreadUpdated(ThreadUpdated),
    TaskAccepted(TaskAccepted),
    TaskRejected(TaskRejected),
    ExecutionFailed(ExecutionFailed),
    JobCreated(JobCreated),
    JobUpdated(JobUpdated),
    MessageCreated(MessageCreated),
    LifecycleChanged(LifecycleChanged),
}
