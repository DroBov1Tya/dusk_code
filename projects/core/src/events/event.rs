use super::{
    ExecutionFailed, JobCreated, JobUpdated, LifecycleChanged, MessageCreated, TaskAccepted,
    TaskRejected, ThreadCreated, ThreadUpdated, TurnCreated, TurnUpdated,
};

#[derive(Debug, Clone)]
#[non_exhaustive]
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
    TurnCreated(TurnCreated),
    TurnUpdated(TurnUpdated),
    LifecycleChanged(LifecycleChanged),
}
