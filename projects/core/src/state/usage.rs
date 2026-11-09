//! Durable counts of what the model uses from the context: skills loaded and MCP tools
//! called. One row per item gives the total across every thread; the per-thread rows give
//! one thread's counts. Counts are written when the use happens and never reset.

use anyhow::{Context, Result, ensure};
use sqlx::Row;

use super::State;

/// What one item has been used for, across all threads and in one of them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemUsage {
    pub kind: String,
    /// The name the model calls it by: the skill name, or `server__tool`.
    pub name: String,
    pub uses_total: u64,
    /// Uses in `thread_id`, when the read was for one; zero otherwise.
    pub uses_in_chat: u64,
}

/// Kinds the counters keep, matching the CHECK in the migration.
pub const KIND_SKILL: &str = "skill";
pub const KIND_MCP_TOOL: &str = "mcp tool";

const KINDS: [&str; 2] = [KIND_SKILL, KIND_MCP_TOOL];

impl State {
    /// Notes one use of `kind` named `name`, in `thread_id` when there is one.
    pub async fn record_item_use(
        &self,
        thread_id: Option<&str>,
        kind: &str,
        name: &str,
    ) -> Result<()> {
        ensure!(KINDS.contains(&kind), "unknown usage kind {kind:?}");
        ensure!(!name.trim().is_empty(), "usage needs a name");
        let mut transaction = self.pool.begin().await.context("begin item use")?;
        // The insert stands on its own; only the count updates ride in the transaction.
        sqlx::query(
            "INSERT INTO item_usage (kind, name, uses_total) VALUES (?, ?, 0)
             ON CONFLICT (kind, name) DO NOTHING",
        )
        .bind(kind)
        .bind(name)
        .execute(&mut *transaction)
        .await
        .context("add the item usage row")?;
        sqlx::query(
            "UPDATE item_usage SET uses_total = uses_total + 1 WHERE kind = ? AND name = ?",
        )
        .bind(kind)
        .bind(name)
        .execute(&mut *transaction)
        .await
        .context("count the item use")?;
        if let Some(thread_id) = thread_id {
            sqlx::query(
                "INSERT INTO thread_item_usage (thread_id, kind, name, uses) VALUES (?, ?, ?, 1)
                 ON CONFLICT (thread_id, kind, name) DO UPDATE SET uses = uses + 1",
            )
            .bind(thread_id)
            .bind(kind)
            .bind(name)
            .execute(&mut *transaction)
            .await
            .context("count the item use in the thread")?;
        }
        transaction.commit().await.context("commit item use")?;
        Ok(())
    }

    /// Every counted item; `thread_id` fills the per-thread counts, and without it
    /// they stay zero.
    pub async fn item_usage(&self, thread_id: Option<&str>) -> Result<Vec<ItemUsage>> {
        let rows = sqlx::query(
            "SELECT i.kind, i.name, i.uses_total,
                    COALESCE(t.uses, 0) AS uses_in_chat
             FROM item_usage i
             LEFT JOIN thread_item_usage t
               ON t.kind = i.kind AND t.name = i.name AND t.thread_id = ?
             ORDER BY i.kind, i.name",
        )
        .bind(thread_id)
        .fetch_all(&self.pool)
        .await
        .context("read item usage")?;
        Ok(rows
            .iter()
            .map(|row| ItemUsage {
                kind: row.get("kind"),
                name: row.get("name"),
                uses_total: row.get::<i64, _>("uses_total") as u64,
                uses_in_chat: row.get::<i64, _>("uses_in_chat") as u64,
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn the_counts_survive_and_split_by_thread() {
        let path = crate::testing::temp_dir("item_usage").join("state.db");
        let state = State::open(&path).await.unwrap();
        state.create_thread("one", "tui").await.unwrap();
        state.create_thread("two", "tui").await.unwrap();

        state
            .record_item_use(Some("one"), "skill", "debugging")
            .await
            .unwrap();
        state
            .record_item_use(Some("two"), "skill", "debugging")
            .await
            .unwrap();
        state
            .record_item_use(Some("one"), "skill", "debugging")
            .await
            .unwrap();
        state
            .record_item_use(Some("two"), "mcp tool", "srv__echo")
            .await
            .unwrap();
        // A use outside any thread counts to the total only.
        state
            .record_item_use(None, "skill", "debugging")
            .await
            .unwrap();

        let one = state.item_usage(Some("one")).await.unwrap();
        let debugging = one.iter().find(|usage| usage.name == "debugging").unwrap();
        // three uses in threads plus the one outside any thread.
        assert_eq!(debugging.uses_total, 4);
        assert_eq!(debugging.uses_in_chat, 2);
        assert_eq!(
            one.iter()
                .find(|u| u.name == "srv__echo")
                .unwrap()
                .uses_in_chat,
            0
        );

        let two = state.item_usage(Some("two")).await.unwrap();
        assert_eq!(
            two.iter()
                .find(|u| u.name == "debugging")
                .unwrap()
                .uses_in_chat,
            1
        );

        state.close().await;
        // Reopen the file: the counts are durable, not in-memory.
        let reopened = State::open(&path).await.unwrap();
        let all = reopened.item_usage(None).await.unwrap();
        assert_eq!(
            all.iter()
                .find(|usage| usage.name == "debugging")
                .unwrap()
                .uses_total,
            4
        );
    }
}
