//! The model an asker picked for its subagent by tier, kept with the thread so the agent
//! answers on it each time it is woken.

use anyhow::{Context, Result};

use super::State;

impl State {
    /// `provider/model`; `None` follows the chat's model.
    pub async fn agent_model(&self, thread_id: &str) -> Result<Option<String>> {
        sqlx::query_scalar("SELECT model FROM agent_models WHERE thread_id = ?")
            .bind(thread_id)
            .fetch_optional(&self.pool)
            .await
            .context("read the agent's model")
    }

    /// `None` puts the agent back on the chat's model.
    pub async fn set_agent_model(&self, thread_id: &str, model: Option<&str>) -> Result<()> {
        let query = match model {
            Some(model) => sqlx::query(
                "INSERT INTO agent_models (thread_id, model) VALUES (?, ?)
                 ON CONFLICT (thread_id) DO UPDATE SET model = excluded.model",
            )
            .bind(thread_id)
            .bind(model),
            None => sqlx::query("DELETE FROM agent_models WHERE thread_id = ?").bind(thread_id),
        };
        query
            .execute(&self.pool)
            .await
            .context("store the agent's model")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::testing::temp_state;

    #[tokio::test]
    async fn an_agent_keeps_its_model_until_put_back_on_the_chats() {
        let state = temp_state("agent_models").await;
        state.create_thread("agent", "test").await.unwrap();
        assert_eq!(state.agent_model("agent").await.unwrap(), None);
        state
            .set_agent_model("agent", Some("big/fable"))
            .await
            .unwrap();
        state
            .set_agent_model("agent", Some("small/haiku"))
            .await
            .unwrap();
        assert_eq!(
            state.agent_model("agent").await.unwrap().as_deref(),
            Some("small/haiku")
        );
        state.set_agent_model("agent", None).await.unwrap();
        assert_eq!(state.agent_model("agent").await.unwrap(), None);
    }
}
