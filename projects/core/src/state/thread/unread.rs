//! Whether a chat holds something its user has not seen. The core sets it when something
//! for the user arrives (`client::serve::unread` says what does); a client that shows the
//! chat clears it. An agent's thread is never unread: only a chat has a user.

use anyhow::{Context, Result};

use super::State;

impl State {
    /// Something for the user arrived in the chat. True when it was read before, so the
    /// clients are told once.
    pub async fn mark_unread(&self, thread_id: &str) -> Result<bool> {
        self.set_unread(thread_id, true).await
    }

    /// A client showed the chat to its user. True when it was unread.
    pub async fn mark_seen(&self, thread_id: &str) -> Result<bool> {
        self.set_unread(thread_id, false).await
    }

    async fn set_unread(&self, thread_id: &str, unread: bool) -> Result<bool> {
        let changed = sqlx::query(
            "UPDATE threads SET unread = ?1
             WHERE id = ?2 AND parent_thread_id IS NULL AND unread != ?1",
        )
        .bind(unread)
        .bind(thread_id)
        .execute(&self.pool)
        .await
        .context("store whether the chat is read")?
        .rows_affected();
        Ok(changed == 1)
    }
}

#[cfg(test)]
mod tests {
    use crate::state::State;
    use crate::testing::temp_dir;

    async fn unread(state: &State, id: &str) -> bool {
        state.load_thread(id).await.unwrap().unwrap().unread
    }

    #[tokio::test]
    async fn a_chat_stays_unread_until_seen_and_across_a_restart() {
        let path = temp_dir("thread_unread").join("state.db");
        let state = State::open(&path).await.unwrap();
        state.create_thread("chat", "test").await.unwrap();
        assert!(!unread(&state, "chat").await, "a new chat starts read");
        assert!(state.mark_unread("chat").await.unwrap());
        assert!(!state.mark_unread("chat").await.unwrap(), "told once");
        let listed = state.list_threads().await.unwrap();
        assert!(
            listed
                .iter()
                .any(|thread| thread.id == "chat" && thread.unread)
        );
        state.close().await;

        let state = State::open(&path).await.unwrap();
        assert!(unread(&state, "chat").await, "kept across a restart");
        assert!(state.mark_seen("chat").await.unwrap());
        assert!(!state.mark_seen("chat").await.unwrap());
        assert!(!unread(&state, "chat").await);
        state.close().await;
    }

    #[tokio::test]
    async fn an_agent_thread_is_never_unread() {
        let state = crate::testing::temp_state("agent_unread").await;
        state.create_thread("chat", "test").await.unwrap();
        state
            .create_agent_thread("agent", "chat", "general", "Aster-01")
            .await
            .unwrap();
        assert!(!state.mark_unread("agent").await.unwrap());
        assert!(!unread(&state, "agent").await);
        assert!(!unread(&state, "chat").await);
        assert!(!state.mark_unread("unknown").await.unwrap());
    }
}
