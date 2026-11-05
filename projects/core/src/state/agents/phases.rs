//! A subagent's phase. Work goes planning, executing, reviewing, done; it can pause from
//! any phase and go on from a pause. The order is enforced, so a phase on record says
//! what really happened, and after a crash an agent is found paused where it stopped.

use anyhow::{Context, Result, bail};

use super::{State, timestamp_ms};

pub use dusk_protocol::agent::AgentPhase;

impl State {
    pub async fn agent_phase(&self, thread_id: &str) -> Result<Option<AgentPhase>> {
        let phase: Option<String> =
            sqlx::query_scalar("SELECT phase FROM agent_phases WHERE thread_id = ?")
                .bind(thread_id)
                .fetch_optional(&self.pool)
                .await
                .context("read the agent's phase")?;
        phase.as_deref().map(AgentPhase::parse).transpose()
    }

    /// Moves an agent on, refusing a step its current phase does not allow.
    pub async fn move_agent_phase(
        &self,
        thread_id: &str,
        to: AgentPhase,
        note: &str,
    ) -> Result<()> {
        let from = self.agent_phase(thread_id).await?;
        if from != Some(to) && !AgentPhase::next(from).contains(&to) {
            let allowed: Vec<&str> = AgentPhase::next(from).iter().map(|p| p.name()).collect();
            bail!(
                "cannot go from {} to {}; next can be {}",
                from.map_or("no phase", AgentPhase::name),
                to.name(),
                allowed.join(", ")
            );
        }
        self.set_agent_phase(thread_id, to, note).await
    }

    /// Records a phase without the order check, for the runtime's own moves.
    pub async fn set_agent_phase(&self, thread_id: &str, to: AgentPhase, note: &str) -> Result<()> {
        sqlx::query(
            "INSERT INTO agent_phases (thread_id, phase, note, updated_at) VALUES (?, ?, ?, ?)
             ON CONFLICT (thread_id) DO UPDATE
             SET phase = excluded.phase, note = excluded.note, updated_at = excluded.updated_at",
        )
        .bind(thread_id)
        .bind(to.name())
        .bind(note.trim())
        .bind(timestamp_ms()?)
        .execute(&self.pool)
        .await
        .context("store the agent's phase")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::temp_state;

    #[tokio::test]
    async fn phases_follow_their_order() {
        let state = temp_state("agent_phases").await;
        state.create_thread("agent", "test").await.unwrap();
        let go = |to| state.move_agent_phase("agent", to, "");
        go(AgentPhase::Planning).await.unwrap();
        let skipped = go(AgentPhase::Done).await.unwrap_err().to_string();
        assert!(
            skipped.contains("next can be executing, paused"),
            "{skipped}"
        );
        go(AgentPhase::Executing).await.unwrap();
        go(AgentPhase::Paused).await.unwrap();
        go(AgentPhase::Reviewing).await.unwrap();
        go(AgentPhase::Done).await.unwrap();
        assert_eq!(
            state.agent_phase("agent").await.unwrap(),
            Some(AgentPhase::Done)
        );
    }
}
