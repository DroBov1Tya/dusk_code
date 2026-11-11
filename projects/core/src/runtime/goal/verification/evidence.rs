//! The evidence saved for a criterion, read back for the review.

use anyhow::{Context, Result};
use sqlx::{Row, Sqlite, Transaction};

pub(super) struct SavedEvidence {
    pub(super) command: Option<String>,
    pub(super) command_passed: bool,
}

pub(super) async fn read_evidence(
    tx: &mut Transaction<'_, Sqlite>,
    thread_id: &str,
    revision: &str,
    after: i64,
    approved_at: i64,
    epoch: i64,
    id: &str,
) -> Result<Option<SavedEvidence>> {
    if let Some(id) = id.strip_prefix("tool:") {
        let row = sqlx::query(
            "SELECT e.tool_name, e.arguments_json, e.exit_code FROM tool_executions e
            JOIN messages m ON m.message_id = e.result_message_id AND m.thread_id = e.thread_id
            JOIN turns t ON t.turn_id = e.turn_id AND t.thread_id = e.thread_id
            WHERE e.thread_id = ? AND e.goal_revision = ? AND e.result_message_id = ?
                AND e.outcome = 'succeeded' AND t.status = 'completed' AND m.sequence > ? AND e.goal_epoch = ?",
        )
        .bind(thread_id)
        .bind(revision)
        .bind(id)
        .bind(after)
        .bind(epoch)
        .fetch_optional(&mut **tx)
        .await?;
        let Some(row) = row else {
            return Ok(None);
        };
        let arguments: serde_json::Value =
            serde_json::from_str(row.try_get("arguments_json")?).context("saved tool arguments")?;
        let shell = row.try_get::<&str, _>("tool_name")? == "run_command"
            && !arguments
                .get("background")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
        Ok(Some(SavedEvidence {
            command: shell
                .then(|| arguments["command"].as_str().map(str::to_owned))
                .flatten(),
            command_passed: shell && row.try_get::<Option<i64>, _>("exit_code")? == Some(0),
        }))
    } else if let Some(id) = id.strip_prefix("job:") {
        let command: Option<String> = sqlx::query_scalar("SELECT command FROM background_jobs
            WHERE thread_id = ? AND goal_revision = ? AND job_id = ? AND status = 'completed' AND created_at >= ? AND goal_epoch = ?")
            .bind(thread_id).bind(revision).bind(id).bind(approved_at).bind(epoch).fetch_optional(&mut **tx).await?;
        Ok(command.map(|command| SavedEvidence {
            command: Some(command),
            command_passed: true,
        }))
    } else {
        Ok(None)
    }
}
