//! What a goal waits on before its turn is judged: a message to take in, or work in the
//! background, such as a running job, an agent at work or a question to the user.

use anyhow::{Context, Result};

use super::State;
use crate::state::thread::threads::below;

impl State {
    /// Pending input and running work in one snapshot. Agent requests may be waiting for
    /// a slot, admitted but not started yet, or in a running turn.
    pub async fn goal_wake_state(&self, thread_id: &str) -> Result<(bool, bool)> {
        sqlx::query_as(below!(
            "SELECT EXISTS(SELECT 1 FROM pending_messages WHERE thread_id = ?1),
                EXISTS(SELECT 1 FROM background_jobs WHERE thread_id = ?1 AND status = 'running')
                OR EXISTS(SELECT 1 FROM pending_messages JOIN below ON thread_id = below.id)
                OR EXISTS(SELECT 1 FROM turns JOIN below ON thread_id = below.id
                    WHERE status = 'running')
                OR EXISTS(SELECT 1 FROM goals WHERE thread_id = ?1 AND awaiting_user = 1)
                OR EXISTS(SELECT 1 FROM messages JOIN below ON thread_id = below.id
                    WHERE (role = 'user' OR from_job_id IS NOT NULL)
                    AND sequence > (SELECT COALESCE(MAX(last_sequence), 0) FROM turns
                        WHERE turns.thread_id = below.id))"
        ))
        .bind(thread_id)
        .fetch_one(&self.pool)
        .await
        .context("check goal dependency wake state")
    }
}
