//! Live threads by ID, so one thread can reach another: wake it for a message, stop it,
//! or pass events on to its client. Entries are weak, so they never keep a thread alive;
//! each runner adds itself before it starts and takes itself out when it ends.

use std::{
    collections::HashMap,
    sync::atomic::{AtomicBool, Ordering},
};

use tokio::sync::{Mutex, MutexGuard, mpsc::WeakUnboundedSender};

use super::Input;
use crate::runtime::ThreadEvent;

#[derive(Default)]
pub struct Mailboxes {
    live: Mutex<HashMap<String, Mailbox>>,
    /// A runner can leave before its final events have reached the asking thread.
    /// Count overlapping runners of the same agent until their reports are drained.
    agent_reports: Mutex<HashMap<String, usize>>,
    /// Held from counting the agents at work to starting a new one, so agents asked
    /// at once cannot all pass the limit.
    pub new_agents: Mutex<()>,
    /// Set at shutdown: messages are still stored, but no agent starts.
    closed: AtomicBool,
}

#[derive(Clone)]
pub struct Mailbox {
    pub input: WeakUnboundedSender<Input>,
    pub events: WeakUnboundedSender<ThreadEvent>,
    /// A subagent's runner rather than a chat's.
    pub agent: bool,
}

impl Mailboxes {
    /// Held while deciding whether a thread runs, so a message and a runner that is
    /// about to end never miss each other.
    pub async fn lock(&self) -> MutexGuard<'_, HashMap<String, Mailbox>> {
        self.live.lock().await
    }

    pub async fn get(&self, thread_id: &str) -> Option<Mailbox> {
        self.lock().await.get(thread_id).cloned()
    }

    pub async fn remove(&self, thread_id: &str) {
        self.lock().await.remove(thread_id);
    }

    /// The caller holds `live`, so starting work and a goal checking it cannot race.
    pub async fn begin_agent_report(&self, thread_id: &str) {
        let mut reports = self.agent_reports.lock().await;
        *reports.entry(thread_id.to_owned()).or_default() += 1;
    }

    /// The caller holds `live`; a newer runner's report keeps the agent active.
    pub async fn end_agent_report(&self, thread_id: &str) {
        let mut reports = self.agent_reports.lock().await;
        if let Some(count) = reports.get_mut(thread_id) {
            *count -= 1;
            if *count == 0 {
                reports.remove(thread_id);
            }
        }
    }

    /// Includes execution and the delivery of its final result, beyond mailbox removal.
    pub async fn has_agent_reports(&self, threads: &[String]) -> bool {
        let reports = self.agent_reports.lock().await;
        threads.iter().any(|thread| reports.contains_key(thread))
    }

    pub fn close(&self) {
        self.closed.store(true, Ordering::SeqCst);
    }

    pub fn is_closed(&self) -> bool {
        self.closed.load(Ordering::SeqCst)
    }
}

impl Mailbox {
    /// True while the runner is there to read its input.
    pub fn is_live(&self) -> bool {
        self.input.strong_count() > 0
    }

    /// False when the thread has ended.
    pub fn send(&self, input: Input) -> bool {
        self.input
            .upgrade()
            .is_some_and(|sender| sender.send(input).is_ok())
    }

    pub fn emit(&self, event: ThreadEvent) {
        if let Some(events) = self.events.upgrade() {
            // A client that stopped listening does not stop the sender.
            let _ = events.send(event);
        }
    }
}
