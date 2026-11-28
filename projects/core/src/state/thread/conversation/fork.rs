//! A fork from one message: a new thread's history copied from another's up to it.

use anyhow::{Context, Result, ensure};
use sqlx::Row;
use uuid::Uuid;

use super::{State, messages::estimate_tokens};

/// Copies the messages `?3` lists, as `[sequence, new id]` pairs in order, from thread
/// `?2` into `?1`, numbered from 1. The pairs drive the join, so each message is one
/// lookup by (thread_id, sequence).
const COPY_MESSAGES: &str = "INSERT INTO messages (message_id, thread_id, sequence, role, content,
        reasoning, tool_calls_json, tool_call_id, content_tokens, tool_calls_tokens,
        reasoning_tokens, created_at, from_thread_id, artifacts_json, goal_verdict)
     SELECT json_extract(pair.value, '$[1]'), ?1, pair.key + 1, m.role, m.content, m.reasoning,
        m.tool_calls_json, m.tool_call_id, m.content_tokens, m.tool_calls_tokens,
        m.reasoning_tokens, m.created_at, m.from_thread_id, m.artifacts_json, m.goal_verdict
     FROM json_each(?3) pair CROSS JOIN messages m
     WHERE m.thread_id = ?2 AND m.sequence = json_extract(pair.value, '$[0]')
     ORDER BY pair.key";

impl State {
    /// Copies `source`'s history up to and including `message_id` into `target`, which
    /// holds nothing yet, in one transaction. A compaction before the cut is carried as
    /// the target's own: its summary, then only the messages after it. The tool results
    /// that answer the chosen message come along, so no call is left unanswered. Job
    /// results are left out: their job belongs to the source. Returns the messages copied.
    pub async fn copy_history(&self, source: &str, target: &str, message_id: &str) -> Result<u64> {
        let mut tx = self.pool.begin().await.context("begin fork")?;
        let chosen: Option<i64> = sqlx::query_scalar(
            "SELECT sequence FROM messages WHERE thread_id = ? AND message_id = ?",
        )
        .bind(source)
        .bind(message_id)
        .fetch_optional(&mut *tx)
        .await
        .context("find the fork's message")?;
        let chosen = chosen.context("no such message in the thread")?;
        let cut: i64 = sqlx::query_scalar(
            "SELECT COALESCE(
                (SELECT MIN(sequence) - 1 FROM messages
                 WHERE thread_id = ? AND sequence > ? AND role != 'tool'),
                (SELECT MAX(sequence) FROM messages WHERE thread_id = ?))",
        )
        .bind(source)
        .bind(chosen)
        .bind(source)
        .fetch_one(&mut *tx)
        .await
        .context("find the fork's end")?;
        let compaction = sqlx::query(
            "SELECT through_sequence, summary FROM compactions
             WHERE thread_id = ? AND through_sequence < ?
             ORDER BY through_sequence DESC, compaction_id DESC LIMIT 1",
        )
        .bind(source)
        .bind(chosen)
        .fetch_optional(&mut *tx)
        .await
        .context("find the fork's compaction")?;
        let (through, summary) = match compaction {
            Some(row) => (
                row.try_get::<i64, _>("through_sequence")?,
                Some(row.try_get::<String, _>("summary")?),
            ),
            None => (0, None),
        };
        // The system prompts, then what the summary does not cover.
        let sequences: Vec<i64> = sqlx::query_scalar(
            "SELECT sequence FROM messages WHERE thread_id = ? AND from_job_id IS NULL
             AND ((role = 'system' AND goal_verdict = 0 AND sequence <= ?)
                OR (sequence > ? AND sequence <= ?))
             ORDER BY sequence",
        )
        .bind(source)
        .bind(through)
        .bind(through)
        .bind(cut)
        .fetch_all(&mut *tx)
        .await
        .context("list the fork's messages")?;
        ensure!(!sequences.is_empty(), "nothing to fork");
        // The copies are numbered from 1 in the source's order, so the summary covers as
        // many of them as there are copied messages it covered.
        let summary_after = i64::try_from(sequences.iter().filter(|s| **s <= through).count())
            .context("too many messages")?;
        // Each copied sequence with a fresh id, made in order so the ids sort as the copies
        // do. The copies are found from these pairs by (thread_id, sequence).
        let pairs: Vec<(i64, String)> = sequences
            .iter()
            .map(|sequence| (*sequence, Uuid::now_v7().to_string()))
            .collect();
        let pairs = serde_json::to_string(&pairs).context("list the copies")?;
        let copied = sqlx::query(COPY_MESSAGES)
            .bind(target)
            .bind(source)
            .bind(&pairs)
            .execute(&mut *tx)
            .await
            .context("copy the messages")?
            .rows_affected();
        ensure!(
            copied == sequences.len() as u64,
            "the fork's messages changed while copying"
        );
        if let Some(summary) = summary {
            // Covers the copied system prompts, as the source's covered its own.
            ensure!(
                summary_after > 0,
                "the compacted thread has no system prompt"
            );
            sqlx::query(
                "INSERT INTO compactions (thread_id, through_sequence, summary, summary_tokens)
                 VALUES (?, ?, ?, ?)",
            )
            .bind(target)
            .bind(summary_after)
            .bind(&summary)
            .bind(i64::try_from(estimate_tokens(&summary)).context("summary is too large")?)
            .execute(&mut *tx)
            .await
            .context("copy the compaction")?;
        }
        let created: i64 = sqlx::query_scalar(
            "SELECT created_at FROM messages WHERE thread_id = ? AND sequence = ?",
        )
        .bind(source)
        .bind(cut)
        .fetch_one(&mut *tx)
        .await
        .context("read the fork's time")?;
        sqlx::query(
            "INSERT INTO sources (source_id, thread_id, name, file, bytes, pinned, delivered, created_at,
                 plan_written_at)
             SELECT lower(hex(randomblob(16))), ?, name, file, bytes, pinned, delivered, created_at,
                 plan_written_at
             FROM sources WHERE thread_id = ? AND created_at <= ?",
        )
        .bind(target)
        .bind(source)
        .bind(created)
        .execute(&mut *tx)
        .await
        .context("copy the sources")?;
        tx.commit().await.context("commit fork")?;
        Ok(copied)
    }
}

#[cfg(test)]
mod tests {
    use crate::{state::State, testing::temp_state};

    async fn texts(state: &State, thread_id: &str) -> Vec<String> {
        let messages = state.list_messages(thread_id).await.unwrap();
        messages.into_iter().map(|m| m.content).collect()
    }

    #[tokio::test]
    async fn a_fork_finds_each_message_by_index() {
        let state = temp_state("fork_plan").await;
        let sql = format!("EXPLAIN QUERY PLAN {}", super::COPY_MESSAGES);
        let plan = crate::testing::query_plan(&state, sql.leak()).await;
        assert!(plan.contains("SEARCH m USING"), "{plan}");
        assert!(!plan.contains("SCAN m"), "{plan}");
    }

    #[tokio::test]
    async fn a_fork_copies_up_to_the_message_and_starts_from_a_compaction_before_it() {
        let state = temp_state("fork").await;
        state.create_thread("source", "test").await.unwrap();
        state.add_system_message("source", "prompt").await.unwrap();
        let mut ids = Vec::new();
        for n in 1..=5 {
            let message = state
                .add_user_message("source", &format!("uid-{n}"), &format!("m{n}"))
                .await
                .unwrap();
            ids.push(message.message_id);
        }

        state.create_thread("plain", "test").await.unwrap();
        assert_eq!(
            state
                .copy_history("source", "plain", &ids[2])
                .await
                .unwrap(),
            4
        );
        assert_eq!(texts(&state, "plain").await, ["prompt", "m1", "m2", "m3"]);
        // Numbered from 1, with fresh ids that sort as the copies do.
        let copies = state.list_messages("plain").await.unwrap();
        let sequences: Vec<u64> = copies.iter().map(|m| m.sequence).collect();
        assert_eq!(sequences, [1, 2, 3, 4]);
        assert!(copies.windows(2).all(|w| w[0].message_id < w[1].message_id));
        assert!(copies.iter().all(|m| !ids.contains(&m.message_id)));
        assert!(state.latest_compaction("plain").await.unwrap().is_none());

        // The summary covers m1 and m2 (sequences 2 and 3); m4 is chosen.
        state.add_compaction("source", 3, "summary").await.unwrap();
        state.create_thread("compacted", "test").await.unwrap();
        state
            .copy_history("source", "compacted", &ids[3])
            .await
            .unwrap();
        assert_eq!(texts(&state, "compacted").await, ["prompt", "m3", "m4"]);
        let compaction = state.latest_compaction("compacted").await.unwrap().unwrap();
        assert_eq!(
            (compaction.through_sequence, compaction.summary.as_str()),
            (1, "summary")
        );

        // A compaction after the chosen message is not used.
        state.create_thread("early", "test").await.unwrap();
        state
            .copy_history("source", "early", &ids[0])
            .await
            .unwrap();
        assert_eq!(texts(&state, "early").await, ["prompt", "m1"]);
        assert!(state.latest_compaction("early").await.unwrap().is_none());
    }
}
