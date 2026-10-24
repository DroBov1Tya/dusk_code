use super::{
    ExecutionFailed, LifecycleChanged, TaskAccepted, TaskRejected, ThreadCreated, ThreadUpdated,
};

#[derive(Debug, Clone)]
pub enum Event {
    ThreadCreated(ThreadCreated),
    ThreadUpdated(ThreadUpdated),
    TaskAccepted(TaskAccepted),
    TaskRejected(TaskRejected),
    ExecutionFailed(ExecutionFailed),
    LifecycleChanged(LifecycleChanged),
}
