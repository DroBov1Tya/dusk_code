//! Live threads by ID, so one thread can reach another: wake it for a message, stop it,
//! or pass events on to its client. The channels are weak, so an entry never keeps a
//! thread alive; each runner adds itself before it starts and takes itself out when it ends.

use std::{
    collections::HashMap,
    sync::atomic::{AtomicBool, Ordering},
};

use anyhow::Result;
use tokio::sync::{Mutex, MutexGuard, mpsc::WeakUnboundedSender};

use super::Input;
use crate::{
    runtime::{ThreadEvent, thread::ContextSource},
    state::State,
};

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
    /// What the runner counts its context with, for a report that agrees with its events.
    pub context: ContextSource,
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

    /// An agent below `root` works or still reports its end, so the chat is at work.
    pub async fn agents_at_work(&self, state: &State, root: &str) -> Result<bool> {
        let below = state.descendant_threads(root).await?;
        Ok(self.has_agent_reports(&below).await)
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

#[cfg(test)]
mod tests {
    use super::Mailboxes;
    use crate::testing::temp_state;

    #[tokio::test]
    async fn a_chat_is_at_work_while_an_agent_at_any_depth_below_it_reports() {
        let state = temp_state("agents_at_work").await;
        state.create_thread("chat", "test").await.unwrap();
        state
            .create_agent_thread("a", "chat", "general", "Aster-01")
            .await
            .unwrap();
        state
            .create_agent_thread("b", "a", "general", "Beryl-02")
            .await
            .unwrap();
        let mailboxes = Mailboxes::default();
        assert!(!mailboxes.agents_at_work(&state, "chat").await.unwrap());

        mailboxes.begin_agent_report("b").await;
        assert!(mailboxes.agents_at_work(&state, "chat").await.unwrap());

        mailboxes.end_agent_report("b").await;
        assert!(!mailboxes.agents_at_work(&state, "chat").await.unwrap());
    }
}
