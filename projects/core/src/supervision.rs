//! Tokio primitives reserved for cooperative cancellation and owned task lifetimes.

use anyhow::Result;
use tokio::{sync::watch, task::JoinSet};

pub type SupervisedTasks = JoinSet<Result<()>>;

/// A retained `true` value requests shutdown; task loops must observe it.
pub type CancellationSender = watch::Sender<bool>;
pub type CancellationReceiver = watch::Receiver<bool>;

// Shutdown deadlines, joining, and error escalation belong to the supervisor implementation.
