//! Taking in what clients and other agents sent: messages join the thread, commands run.

use std::sync::Arc;

use anyhow::Result;

use super::ThreadRunner;
use crate::runtime::{ThreadEvent, agent::Admit, goal::command::GoalAction};
use crate::{message::commands, state::PendingMessage};
use dusk_protocol::client::Command;

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
            return self.agent.admit(&pending, Admit::BETWEEN_TURNS).await;
        };
        match command {
            Err(error) => self.agent.emit(ThreadEvent::Rejected {
                reason: format!("{error:#}"),
            }),
            Ok(command) => {
                if let Command::Goal { argument } = &command
                    && GoalAction::from_command(argument.as_deref())
                        .is_ok_and(|action| action != GoalAction::Show)
                {
                    // A goal set, replaced or cleared starts its counts again: turns of
                    // a goal before it must not stall or use up the retries of this one.
                    self.goal_quiet_turns = 0;
                    self.goal_unjudged_turns = 0;
                    self.goal_retries = 0;
                }
                commands::run_in_thread(command, &self.agent, &mut self.inbox).await
            }
        }
        // Removed only once done: a command cut short by a crash runs again on restart.
        state.remove_pending(pending.pending_id).await
    }
}
