use super::{LifecycleChanged, TaskAccepted, TaskRejected, ThreadCreated, ThreadUpdated};

#[derive(Debug, Clone)]
pub enum Event {
    ThreadCreated(ThreadCreated),
    ThreadUpdated(ThreadUpdated),
    TaskAccepted(TaskAccepted),
    TaskRejected(TaskRejected),
    LifecycleChanged(LifecycleChanged),
}
