//! A chat's own model and effort. The settings keep the last ones chosen, which a new
//! chat starts with; a chat keeps its own, so a choice made in another chat does not
//! change what this one, its agents or its team answer on.

use anyhow::{Context, Result};

use dusk_protocol::model::Effort;

use super::State;

impl State {
    /// The chat's model as `provider/model`; `None` for a chat from before the record, or
    /// one opened while no model was chosen.
    pub async fn chat_model(&self, thread_id: &str) -> Result<Option<String>> {
        let model: Option<Option<String>> =
            sqlx::query_scalar("SELECT model FROM threads WHERE id = ?")
                .bind(thread_id)
                .fetch_optional(&self.pool)
                .await
                .context("read the chat's model")?;
        Ok(model.flatten())
    }

    pub async fn set_chat_model(&self, thread_id: &str, model: &str) -> Result<()> {
        sqlx::query("UPDATE threads SET model = ? WHERE id = ? AND parent_thread_id IS NULL")
            .bind(model)
            .bind(thread_id)
            .execute(&self.pool)
            .await
            .context("store the chat's model")?;
        Ok(())
    }

    /// Chats with no model of their own keep `model` and `effort` from now on: before
    /// chats kept their own, every chat ran on the saved ones.
    pub async fn keep_chat_models(&self, model: &str, effort: Option<Effort>) -> Result<()> {
        sqlx::query(
            "UPDATE threads SET model = COALESCE(model, ?), effort = COALESCE(effort, ?)
             WHERE parent_thread_id IS NULL AND (model IS NULL OR effort IS NULL)",
        )
        .bind(model)
        .bind(effort.map_or("default", Effort::name))
        .execute(&self.pool)
        .await
        .context("keep the chats' models")?;
        Ok(())
    }

    /// The chat's effort: `None` follows the settings, `Some(None)` is the provider's.
    pub async fn chat_effort(&self, thread_id: &str) -> Result<Option<Option<Effort>>> {
        let effort: Option<Option<String>> =
            sqlx::query_scalar("SELECT effort FROM threads WHERE id = ?")
                .bind(thread_id)
                .fetch_optional(&self.pool)
                .await
                .context("read the chat's effort")?;
        effort
            .flatten()
            .map(|name| Effort::parse(&name))
            .transpose()
            .context("read the chat's effort")
    }

    pub async fn set_chat_effort(&self, thread_id: &str, effort: Option<Effort>) -> Result<()> {
        sqlx::query("UPDATE threads SET effort = ? WHERE id = ? AND parent_thread_id IS NULL")
            .bind(effort.map_or("default", Effort::name))
            .bind(thread_id)
            .execute(&self.pool)
            .await
            .context("store the chat's effort")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use dusk_protocol::model::Effort;

    use crate::testing::temp_state;

    #[tokio::test]
    async fn each_chat_keeps_its_own_model_and_effort() {
        let state = temp_state("chat_model").await;
        for id in ["one", "two"] {
            state.create_thread(id, "test").await.unwrap();
        }
        state
            .create_agent_thread("agent", "one", "general", "Ember-0c")
            .await
            .unwrap();
        assert_eq!(state.chat_model("one").await.unwrap(), None);
        assert_eq!(state.chat_effort("one").await.unwrap(), None);
        state.set_chat_model("one", "qwen/coder").await.unwrap();
        state.set_chat_model("two", "deepseek/chat").await.unwrap();
        state
            .set_chat_effort("one", Some(Effort::High))
            .await
            .unwrap();
        state.set_chat_effort("two", None).await.unwrap();
        assert_eq!(
            state.chat_model("one").await.unwrap().as_deref(),
            Some("qwen/coder")
        );
        assert_eq!(
            state.chat_model("two").await.unwrap().as_deref(),
            Some("deepseek/chat")
        );
        assert_eq!(
            state.chat_effort("one").await.unwrap(),
            Some(Some(Effort::High))
        );
        assert_eq!(state.chat_effort("two").await.unwrap(), Some(None));
        // An agent follows its chat and keeps nothing of its own here.
        state.set_chat_model("agent", "other/model").await.unwrap();
        assert_eq!(state.chat_model("agent").await.unwrap(), None);

        // A chat from before the record keeps what every chat ran on; others stay theirs.
        state.create_thread("old", "test").await.unwrap();
        state
            .keep_chat_models("qwen/coder", Some(Effort::Low))
            .await
            .unwrap();
        assert_eq!(
            state.chat_model("old").await.unwrap().as_deref(),
            Some("qwen/coder")
        );
        assert_eq!(
            state.chat_effort("old").await.unwrap(),
            Some(Some(Effort::Low))
        );
        assert_eq!(
            state.chat_model("two").await.unwrap().as_deref(),
            Some("deepseek/chat")
        );
        assert_eq!(state.chat_effort("two").await.unwrap(), Some(None));
        assert_eq!(state.chat_model("agent").await.unwrap(), None);
    }
}
