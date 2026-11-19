//! A chat's own mode. The setting names one mode for every chat; a chat keeps the one its
//! user last wrote to it in, so a switch made for another chat does not change what this
//! one does when the core wakes it.

use anyhow::{Context, Result};

use super::State;

impl State {
    /// The mode the chat's turns take up; `None` before the user first wrote to it.
    pub async fn chat_mode(&self, thread_id: &str) -> Result<Option<String>> {
        let mode: Option<Option<String>> =
            sqlx::query_scalar("SELECT mode FROM threads WHERE id = ?")
                .bind(thread_id)
                .fetch_optional(&self.pool)
                .await
                .context("read the chat's mode")?;
        Ok(mode.flatten())
    }

    /// The user wrote to the chat in `mode`: its turns take that one up from now on.
    pub async fn set_chat_mode(&self, thread_id: &str, mode: &str) -> Result<()> {
        sqlx::query("UPDATE threads SET mode = ? WHERE id = ? AND parent_thread_id IS NULL")
            .bind(mode)
            .bind(thread_id)
            .execute(&self.pool)
            .await
            .context("store the chat's mode")?;
        Ok(())
    }
}
