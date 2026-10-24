//! In-process event contract and Tokio broadcast transport.

use tokio::sync::broadcast;

use crate::lifecycle::LifecycleState;

#[derive(Debug, Clone)]
pub enum CoreEvent {
    LifecycleChanged {
        previous: LifecycleState,
        current: LifecycleState,
    },
}

pub type EventSender = broadcast::Sender<CoreEvent>;
pub type EventReceiver = broadcast::Receiver<CoreEvent>;

// Consumers must handle Tokio broadcast lag; durable delivery is not provided.
