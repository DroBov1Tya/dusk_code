//! Taking in what clients and other agents sent: messages join the thread, commands run.

use std::sync::Arc;

use anyhow::Result;

use super::ThreadRunner;
use crate::runtime::ThreadEvent;
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
        let state = Arc::clone(&self.agent.state);
        // Another agent's message is never a command, whatever it starts with.
        let command = if pending.from_thread_id.is_some() || pending.from_job_id.is_some() {
            None
        } else {
            commands::parse(&pending.content)
        };
        let Some(command) = command else {
            let message = state.admit_pending(&self.agent.thread_id, &pending).await?;
            if message.from_thread_id.is_some() {
                sqlx::query("UPDATE goals SET work_epoch = work_epoch + 1 WHERE thread_id = ? AND approved_at IS NOT NULL")
                    .bind(&self.agent.thread_id).execute(&state.pool).await?;
            } else if message.from_job_id.is_none() {
                sqlx::query("UPDATE goals SET awaiting_user = 0 WHERE thread_id = ?")
                    .bind(&self.agent.thread_id)
                    .execute(&state.pool)
                    .await?;
            }
            if message.from_job_id.is_none() {
                self.agent.title_from_first_message(&message).await?;
            }
            if message.from_thread_id.is_none() && message.from_job_id.is_none() {
                let details = serde_json::json!({ "text": message.content });
                self.agent
                    .hook_notes(crate::plugins::HookEvent::UserMessage, details)
                    .await?;
            }
            self.agent.emit(ThreadEvent::Queued {
                message_id: message.message_id.clone(),
            });
            if message.from_job_id.is_some() {
                self.agent.emit(ThreadEvent::FromJob(message));
            } else if message.from_thread_id.is_some() {
                self.agent.emit(ThreadEvent::FromAgent(message));
            } else {
                self.agent.emit(ThreadEvent::UserMessage(message));
            }
            return Ok(());
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
