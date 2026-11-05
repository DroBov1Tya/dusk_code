//! A thread's own approval mode. No row means the default held in the settings; the
//! mode is read fresh for each tool call, so it takes hold at once.

use anyhow::{Context, Result};

use super::State;
use crate::config::ApprovalPolicy;

impl State {
    /// The thread's own approval mode; `None` when it uses the default in settings.
    pub async fn thread_approvals(&self, thread_id: &str) -> Result<Option<ApprovalPolicy>> {
        let policy: Option<String> =
            sqlx::query_scalar("SELECT policy FROM thread_approvals WHERE thread_id = ?")
                .bind(thread_id)
                .fetch_optional(&self.pool)
                .await
                .context("read the thread's approval mode")?;
        policy
            .as_deref()
            .map(ApprovalPolicy::parse_stored)
            .transpose()
    }

    /// Sets the thread's own approval mode; `None` goes back to the default in settings.
    pub async fn set_thread_approvals(
        &self,
        thread_id: &str,
        policy: Option<ApprovalPolicy>,
    ) -> Result<()> {
        match policy {
            Some(policy) => {
                sqlx::query(
                    "INSERT INTO thread_approvals (thread_id, policy) VALUES (?, ?)
                     ON CONFLICT (thread_id) DO UPDATE SET policy = excluded.policy",
                )
                .bind(thread_id)
                .bind(policy.stored_name())
                .execute(&self.pool)
                .await
                .context("store the thread's approval mode")?;
            }
            None => {
                sqlx::query("DELETE FROM thread_approvals WHERE thread_id = ?")
                    .bind(thread_id)
                    .execute(&self.pool)
                    .await
                    .context("drop the thread's approval mode")?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::{config::ApprovalPolicy, testing::temp_state};

    #[tokio::test]
    async fn a_thread_keeps_its_own_approval_mode() {
        let state = temp_state("thread_approvals").await;
        state.create_thread("chat", "test").await.unwrap();
        assert_eq!(state.thread_approvals("chat").await.unwrap(), None);
        for policy in [
            ApprovalPolicy::Sandbox,
            ApprovalPolicy::Arbiter,
            ApprovalPolicy::FullAccess,
            ApprovalPolicy::Ask,
        ] {
            state
                .set_thread_approvals("chat", Some(policy))
                .await
                .unwrap();
            assert_eq!(state.thread_approvals("chat").await.unwrap(), Some(policy));
        }
        // Opening the settings default again takes the override away.
        state.set_thread_approvals("chat", None).await.unwrap();
        assert_eq!(state.thread_approvals("chat").await.unwrap(), None);
    }
}
