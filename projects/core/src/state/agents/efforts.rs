//! The reasoning effort an asker chose for its subagent, kept with the thread so the
//! agent thinks the same way each time it is woken.

use anyhow::{Context, Result};

use super::State;
use crate::config::Effort;

impl State {
    pub async fn agent_effort(&self, thread_id: &str) -> Result<Option<Effort>> {
        let effort: Option<String> =
            sqlx::query_scalar("SELECT effort FROM agent_efforts WHERE thread_id = ?")
                .bind(thread_id)
                .fetch_optional(&self.pool)
                .await
                .context("read the agent's effort")?;
        Ok(effort.as_deref().map(Effort::parse).transpose()?.flatten())
    }

    pub async fn set_agent_effort(&self, thread_id: &str, effort: Effort) -> Result<()> {
        sqlx::query(
            "INSERT INTO agent_efforts (thread_id, effort) VALUES (?, ?)
             ON CONFLICT (thread_id) DO UPDATE SET effort = excluded.effort",
        )
        .bind(thread_id)
        .bind(effort.name())
        .execute(&self.pool)
        .await
        .context("store the agent's effort")?;
        Ok(())
    }
}
