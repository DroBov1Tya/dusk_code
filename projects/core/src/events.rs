//! Live events, published after a successful database write. Delivery is in-process only:
//! there is no replay, and slow receivers can lag and miss events.

use tokio::sync::broadcast;

use crate::state::{Execution, Job, Message, ThreadMetadata, Turn};

pub type EventSender = broadcast::Sender<Event>;
pub type EventReceiver = broadcast::Receiver<Event>;

#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum Event {
    ThreadCreated(ThreadMetadata),
    ThreadUpdated(ThreadMetadata),
    TaskAccepted(Execution),
    TaskRejected(Execution),
    MessageCreated(Message),
    TurnStarted(Turn),
    TurnFinished(Turn),
    JobCreated(Job),
    JobUpdated(Job),
}
