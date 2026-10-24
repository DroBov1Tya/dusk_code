use crate::lifecycle::LifecycleState;

#[derive(Debug, Clone)]
pub struct LifecycleChanged {
    pub previous: LifecycleState,
    pub current: LifecycleState,
}
