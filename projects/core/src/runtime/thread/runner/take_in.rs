//! Taking in what clients and other agents sent: messages join the thread, commands run.

use std::sync::Arc;

use anyhow::Result;

use super::ThreadRunner;
use crate::runtime::{ThreadEvent, agent::Admit};
use crate::{message::commands, state::PendingMessage};

impl ThreadRunner {
    /// Takes in what clients sent, oldest first: messages join the thread, commands run.
    /// A command can take a while, so whatever arrived meanwhile is taken in as well.
    pub async fn take_in(&mut self) -> Result<()> {
        loop {
            let pending = self
                .agent
                .state
                .pending_messages(&self.agent.thread_id)
                .await?;
            if pending.is_empty() {
                return Ok(());
            }
            for message in pending {
                self.take(message).await?;
            }
        }
    }

    async fn take(&mut self, pending: PendingMessage) -> Result<()> {
        // The user or another agent wrote with the goal's kickoff: the turn answers
        // them, not the round. A finished job's report is part of the round's work.
        if pending.from_job_id.is_none()
            && self
                .round_queued
                .is_some_and(|kickoff| kickoff != pending.pending_id)
        {
            self.round_queued = None;
        }
        let state = Arc::clone(&self.agent.state);
        // Another agent's message is never a command, whatever it starts with.
        let command = if pending.from_thread_id.is_some() || pending.from_job_id.is_some() {
            None
        } else {
            commands::parse(&pending.content)
        };
        let Some(command) = command else {
            return self.agent.admit(&pending, Admit::BETWEEN_TURNS).await;
        };
        match command {
            Err(error) => self.agent.emit(ThreadEvent::Rejected {
                reason: format!("{error:#}"),
            }),
            Ok(command) => commands::run_in_thread(command, &self.agent, &mut self.inbox).await,
        }
        // Removed only once done: a command cut short by a crash runs again on restart.
        state.remove_pending(pending.pending_id).await
    }
}
