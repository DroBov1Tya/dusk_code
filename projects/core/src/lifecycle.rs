//! Lifecycle vocabulary; allowed transitions will be enforced by the runtime.

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum LifecycleState {
    #[default]
    Created,
    Running,
    Rerun,
    Stopping,
    Stopped,
    Failed,
}
