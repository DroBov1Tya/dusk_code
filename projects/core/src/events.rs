//! In-process event contract and Tokio broadcast transport.

use tokio::sync::broadcast;

use crate::Execution;
use crate::lifecycle::LifecycleState;
use crate::state::ThreadMetadata;

#[derive(Debug, Clone)]
pub enum Event {
    ThreadCreated(ThreadMetadata),
    ThreadUpdated(ThreadMetadata),
    TaskAccepted(Execution),
    TaskRejected(Execution),
    LifecycleChanged {
        previous: LifecycleState,
        current: LifecycleState,
    },
}

/// Compatibility name for existing event consumers.
pub type CoreEvent = Event;

pub type EventSender = broadcast::Sender<CoreEvent>;
pub type EventReceiver = broadcast::Receiver<CoreEvent>;

// Consumers must handle Tokio broadcast lag; durable delivery is not provided.
