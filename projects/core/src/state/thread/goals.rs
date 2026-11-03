//! A thread's standing goal, kept across restarts.

use anyhow::{Context, Result, bail, ensure};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sqlx::{Row, sqlite::SqliteRow};
use uuid::Uuid;

use super::{State, timestamp_ms};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GoalStatus {
    Active,
    Paused,
    Done,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Goal {
    pub thread_id: String,
    pub text: String,
    pub status: GoalStatus,
    pub rounds: u32,
    pub note: Option<String>,
    pub revision: String,
    pub criteria: Vec<GoalCriterion>,
    pub approved: bool,
    pub verification: String,
    pub verification_note: Option<String>,
    pub evidence: Vec<GoalEvidence>,
}

/// All criteria are mandatory. A command proves exactly the approved check's exit;
/// a semantic claim always needs a human's review of the saved result.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GoalCriterion {
    pub id: String,
    pub description: String,
    pub check: GoalCheck,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum GoalCheck {
    Command { command: String },
    UserConfirmation,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GoalEvidence {
    pub criterion_id: String,
    /// tool:<saved result message ID> or job:<saved job ID>.
    pub result_id: String,
}

impl GoalCriterion {
    fn validate(criteria: &[Self]) -> Result<()> {
        ensure!(
            (1..=20).contains(&criteria.len()),
            "provide 1 to 20 mandatory criteria"
        );
        for (index, criterion) in criteria.iter().enumerate() {
            ensure!(
                !criterion.id.is_empty()
                    && criterion.id.len() <= 32
                    && criterion
                        .id
                        .bytes()
                        .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-'),
                "criterion IDs need 1 to 32 ASCII letters, digits, underscores or hyphens"
            );
            ensure!(
                !criteria[..index].iter().any(|c| c.id == criterion.id),
                "duplicate criterion ID"
            );
            ensure!(
                !criterion.description.trim().is_empty() && criterion.description.len() <= 2000,
                "criterion description needs 1 to 2000 bytes"
            );
            if let GoalCheck::Command { command } = &criterion.check {
                ensure!(
                    !command.trim().is_empty() && command.len() <= 16 * 1024,
                    "approved command needs 1 to 16384 bytes"
                );
            }
        }
        Ok(())
    }
}

impl GoalStatus {
    pub fn name(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Paused => "paused",
            Self::Done => "done",
        }
    }

    fn parse(text: &str) -> Result<Self> {
        Ok(match text {
            "active" => Self::Active,
            "paused" => Self::Paused,
            "done" => Self::Done,
            other => bail!("unknown goal status {other}"),
        })
    }
}

impl State {
    /// Pending input and durable dependencies in one snapshot. Agent requests may be
    /// waiting for a slot, admitted but not started yet, or in a running turn.
    pub(crate) async fn goal_wake_state(&self, thread_id: &str) -> Result<(bool, bool)> {
        sqlx::query_as(
            "WITH RECURSIVE agents(id) AS (
                SELECT id FROM threads WHERE parent_thread_id = ?1
                UNION ALL
                SELECT threads.id FROM threads JOIN agents ON threads.parent_thread_id = agents.id
             ) SELECT EXISTS(SELECT 1 FROM pending_messages WHERE thread_id = ?1),
                EXISTS(SELECT 1 FROM background_jobs WHERE thread_id = ?1 AND status = 'running')
                OR EXISTS(SELECT 1 FROM pending_messages JOIN agents ON thread_id = agents.id)
                OR EXISTS(SELECT 1 FROM turns JOIN agents ON thread_id = agents.id
                    WHERE status = 'running')
                OR EXISTS(SELECT 1 FROM goals WHERE thread_id = ?1 AND awaiting_user = 1)
                OR EXISTS(SELECT 1 FROM messages JOIN agents ON thread_id = agents.id
                    WHERE (role = 'user' OR from_job_id IS NOT NULL)
                    AND sequence > (SELECT COALESCE(MAX(last_sequence), 0) FROM turns
                        WHERE turns.thread_id = agents.id))",
        )
        .bind(thread_id)
        .fetch_one(&self.pool)
        .await
        .context("check goal dependency wake state")
    }

    pub async fn goal(&self, thread_id: &str) -> Result<Option<Goal>> {
        let row = sqlx::query("SELECT * FROM goals WHERE thread_id = ?")
            .bind(thread_id)
            .fetch_optional(&self.pool)
            .await
            .context("read goal")?;
        row.map(decode).transpose()
    }

    /// Only user commands replace a goal. A fresh revision invalidates all evidence.
    pub(crate) async fn set_goal(&self, thread_id: &str, text: &str) -> Result<Goal> {
        ensure!(!text.trim().is_empty(), "the goal is empty");
        sqlx::query(
            "INSERT INTO goals (thread_id, text, status, rounds, note, updated_at, revision)
             VALUES (?, ?, 'paused', 0, NULL, ?, ?)
             ON CONFLICT (thread_id) DO UPDATE SET text = excluded.text,
                status = 'paused', rounds = 0, note = NULL, updated_at = excluded.updated_at,
                revision = excluded.revision, criteria_json = '[]', approved_at = NULL, work_epoch = 0, awaiting_user = 0,
                evidence_after_sequence = 0, verification = 'unverified', verification_id = NULL,
                verification_request = NULL, verification_turn_id = NULL,
                verification_attempts = 0, verification_note = NULL",
        )
        .bind(thread_id)
        .bind(text.trim())
        .bind(timestamp_ms()?)
        .bind(Uuid::now_v7().to_string())
        .execute(&self.pool)
        .await
        .context("store draft goal")?;
        self.goal(thread_id).await?.context("goal is gone")
    }

    /// Proposal is allowed only before approval; it receives a new approval token.
    pub(crate) async fn propose_goal(
        &self,
        thread_id: &str,
        revision: &str,
        criteria: Vec<GoalCriterion>,
    ) -> Result<Goal> {
        GoalCriterion::validate(&criteria)?;
        let changed = sqlx::query(
            "UPDATE goals SET criteria_json = ?, revision = ?, updated_at = ?
            WHERE thread_id = ? AND revision = ? AND approved_at IS NULL",
        )
        .bind(serde_json::to_string(&criteria)?)
        .bind(Uuid::now_v7().to_string())
        .bind(timestamp_ms()?)
        .bind(thread_id)
        .bind(revision)
        .execute(&self.pool)
        .await?;
        ensure!(
            changed.rows_affected() == 1,
            "contract changed or is approved; only the user can replace it"
        );
        self.goal(thread_id).await?.context("goal is gone")
    }

    pub(crate) async fn approve_goal(&self, thread_id: &str, revision: &str) -> Result<Goal> {
        let mut tx = self.pool.begin().await?;
        let row = sqlx::query("SELECT * FROM goals WHERE thread_id = ? AND revision = ?")
            .bind(thread_id)
            .bind(revision)
            .fetch_optional(&mut *tx)
            .await?
            .context("goal revision changed; review /goal again")?;
        let goal = decode(row)?;
        ensure!(!goal.approved, "contract is already approved");
        GoalCriterion::validate(&goal.criteria)?;
        sqlx::query("UPDATE goals SET approved_at = ?, status = 'active', rounds = 0,
            evidence_after_sequence = (SELECT COALESCE(MAX(sequence), 0) FROM messages WHERE thread_id = ?),
            updated_at = ? WHERE thread_id = ? AND revision = ?")
            .bind(timestamp_ms()?).bind(thread_id).bind(timestamp_ms()?).bind(thread_id).bind(revision)
            .execute(&mut *tx).await?;
        tx.commit().await?;
        self.goal(thread_id).await?.context("goal is gone")
    }

    /// Public transitions cannot complete a goal. Runtime verification owns Done.
    pub(crate) async fn set_goal_status(
        &self,
        thread_id: &str,
        status: GoalStatus,
        note: Option<&str>,
    ) -> Result<Option<Goal>> {
        ensure!(
            status != GoalStatus::Done,
            "only runtime verification can complete a goal"
        );
        let note = note.map(str::trim).filter(|note| !note.is_empty());
        let changed = sqlx::query(
            "UPDATE goals SET status = ?, note = ?, updated_at = ?,
                rounds = CASE WHEN ? = 'active' THEN 0 ELSE rounds END,
                work_epoch = work_epoch + CASE WHEN ? = 'active' THEN 1 ELSE 0 END,
                verification = CASE WHEN verification = 'passed' THEN 'unverified' ELSE verification END,
                awaiting_user = CASE WHEN ? = 'active' THEN 0 ELSE awaiting_user END,
                verification_attempts = CASE WHEN ? = 'active' THEN 0 ELSE verification_attempts END
             WHERE thread_id = ? AND (? != 'active' OR approved_at IS NOT NULL)",
        ).bind(status.name()).bind(note).bind(timestamp_ms()?).bind(status.name()).bind(status.name()).bind(status.name()).bind(status.name())
            .bind(thread_id).bind(status.name()).execute(&self.pool).await?;
        ensure!(
            changed.rows_affected() == 1,
            "no approved goal; review and approve its criteria first"
        );
        self.goal(thread_id).await
    }

    pub(crate) async fn request_goal_verification(
        &self,
        thread_id: &str,
        revision: &str,
        call_id: &str,
        evidence: Vec<GoalEvidence>,
    ) -> Result<Goal> {
        let mut tx = self.pool.begin().await?;
        let goal = sqlx::query(
            "SELECT * FROM goals WHERE thread_id = ? AND revision = ?
            AND status = 'active' AND approved_at IS NOT NULL",
        )
        .bind(thread_id)
        .bind(revision)
        .fetch_optional(&mut *tx)
        .await?
        .context("no active approved contract at this revision")?;
        let goal = decode(goal)?;
        ensure!(
            evidence.len() == goal.criteria.len(),
            "submit exactly one saved result for every criterion"
        );
        for criterion in &goal.criteria {
            ensure!(
                evidence
                    .iter()
                    .filter(|e| e.criterion_id == criterion.id)
                    .count()
                    == 1,
                "missing or duplicate evidence for {}",
                criterion.id
            );
        }
        ensure!(
            evidence.iter().all(|e| e.result_id.len() <= 128),
            "invalid result ID"
        );
        let turn: String = sqlx::query_scalar(
            "SELECT e.turn_id FROM tool_executions e JOIN turns t USING(turn_id)
            WHERE e.thread_id = ? AND e.call_id = ? AND e.tool_name = 'goal'
                AND e.goal_revision = ? AND t.status = 'running'",
        )
        .bind(thread_id)
        .bind(call_id)
        .bind(revision)
        .fetch_optional(&mut *tx)
        .await?
        .context("verification must be requested by a recorded call in the current goal turn")?;
        let attempts: i64 =
            sqlx::query_scalar("SELECT verification_attempts FROM goals WHERE thread_id = ?")
                .bind(thread_id)
                .fetch_one(&mut *tx)
                .await?;
        ensure!(
            attempts < 3,
            "three verification attempts used; user must review /goal and resume"
        );
        sqlx::query("UPDATE goals SET verification = 'pending', verification_id = ?,
            verification_request = ?, verification_turn_id = ?, verification_note = NULL WHERE thread_id = ?")
            .bind(Uuid::now_v7().to_string()).bind(serde_json::to_string(&evidence)?).bind(turn)
            .bind(thread_id).execute(&mut *tx).await?;
        tx.commit().await?;
        self.goal(thread_id).await?.context("goal is gone")
    }

    pub(crate) async fn confirm_goal(
        &self,
        thread_id: &str,
        revision: &str,
        criterion_id: &str,
        result_id: &str,
    ) -> Result<Goal> {
        let mut tx = self.pool.begin().await?;
        let goal = sqlx::query(
            "SELECT * FROM goals WHERE thread_id = ? AND revision = ? AND approved_at IS NOT NULL",
        )
        .bind(thread_id)
        .bind(revision)
        .fetch_optional(&mut *tx)
        .await?
        .context("approved revision changed; review /goal again")?;
        let goal = decode(goal)?;
        ensure!(
            goal.criteria
                .iter()
                .any(|c| c.id == criterion_id && c.check == GoalCheck::UserConfirmation),
            "criterion is not a user confirmation"
        );
        ensure!(
            goal.evidence
                .iter()
                .any(|e| e.criterion_id == criterion_id && e.result_id == result_id),
            "confirm the exact saved result currently submitted for this criterion"
        );
        sqlx::query("INSERT OR IGNORE INTO goal_confirmations VALUES (?, ?, ?, ?, ?)")
            .bind(thread_id)
            .bind(revision)
            .bind(criterion_id)
            .bind(result_id)
            .bind(timestamp_ms()?)
            .execute(&mut *tx)
            .await?;
        sqlx::query("UPDATE goals SET verification = 'pending', verification_attempts = 0 WHERE thread_id = ?")
            .bind(thread_id).execute(&mut *tx).await?;
        tx.commit().await?;
        self.goal(thread_id).await?.context("goal is gone")
    }

    pub(crate) async fn recover_goal_verification(&self, thread_id: &str) -> Result<()> {
        sqlx::query(
            "UPDATE goals SET verification = 'gaps', verification_note =
            'Verification was interrupted; no pass was recorded. Submit fresh verification.'
            WHERE thread_id = ? AND verification IN ('checking', 'pending')",
        )
        .bind(thread_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub(crate) async fn inspect_goal_result(&self, thread_id: &str, id: &str) -> Result<String> {
        let output: Option<String> = if let Some(id) = id.strip_prefix("tool:") {
            sqlx::query_scalar("SELECT 'tool=' || e.tool_name || ', outcome=' || e.outcome || ', exit=' || COALESCE(e.exit_code, 'unknown') ||
                    ', epoch=' || e.goal_epoch || ', current_epoch=' || g.work_epoch || ', arguments=' || e.arguments_json || char(10) || m.content
                FROM tool_executions e JOIN messages m ON m.message_id = e.result_message_id
                JOIN goals g ON g.thread_id = e.thread_id AND g.revision = e.goal_revision
                WHERE e.thread_id = ? AND e.result_message_id = ?")
                .bind(thread_id).bind(id).fetch_optional(&self.pool).await?
        } else if let Some(id) = id.strip_prefix("job:") {
            sqlx::query_scalar("SELECT 'status=' || b.status || ', command=' || b.command || ', epoch=' || b.goal_epoch ||
                ', current_epoch=' || g.work_epoch || char(10) || b.output FROM background_jobs b JOIN goals g
                ON g.thread_id = b.thread_id AND g.revision = b.goal_revision WHERE b.thread_id = ? AND b.job_id = ?")
                .bind(thread_id).bind(id).fetch_optional(&self.pool).await?
        } else {
            bail!("use a saved tool:<message ID> or job:<job ID>");
        };
        let output = output.context("no saved result for this goal revision and thread")?;
        Ok(format!("{id}\n{output}"))
    }

    /// Bounded index for both the user and model; text itself remains in message history.
    pub(crate) async fn goal_result_index(&self, thread_id: &str) -> Result<String> {
        let rows: Vec<(String, String, String, Option<i64>)> = sqlx::query_as(
            "SELECT 'tool:' || result_message_id, tool_name || ' ' || substr(arguments_json, 1, 300),
                outcome || CASE WHEN goal_epoch != (SELECT work_epoch FROM goals WHERE thread_id = ?1) THEN ' (stale)' ELSE '' END, exit_code FROM tool_executions
             WHERE thread_id = ?1 AND goal_revision = (SELECT revision FROM goals WHERE thread_id = ?1)
                AND result_message_id IS NOT NULL ORDER BY rowid DESC LIMIT 20")
            .bind(thread_id).fetch_all(&self.pool).await?;
        let jobs: Vec<(String, String, String)> = sqlx::query_as("SELECT 'job:' || job_id, command, status
            FROM background_jobs WHERE thread_id = ?1 AND goal_revision = (SELECT revision FROM goals WHERE thread_id = ?1)
            ORDER BY created_at DESC LIMIT 20").bind(thread_id).fetch_all(&self.pool).await?;
        Ok(rows
            .into_iter()
            .map(|(id, tool, outcome, code)| format!("{id}: {tool} {outcome}, exit={code:?}"))
            .chain(
                jobs.into_iter()
                    .map(|(id, command, status)| format!("{id}: {status}, command={command}")),
            )
            .collect::<Vec<_>>()
            .join("\n"))
    }

    pub(crate) async fn count_goal_round(&self, thread_id: &str) -> Result<Option<Goal>> {
        sqlx::query("UPDATE goals SET rounds = rounds + 1, updated_at = ? WHERE thread_id = ?")
            .bind(timestamp_ms()?)
            .bind(thread_id)
            .execute(&self.pool)
            .await
            .context("count goal round")?;
        self.goal(thread_id).await
    }

    /// Whether there was a goal to clear.
    pub(crate) async fn clear_goal(&self, thread_id: &str) -> Result<bool> {
        let done = sqlx::query("DELETE FROM goals WHERE thread_id = ?")
            .bind(thread_id)
            .execute(&self.pool)
            .await
            .context("clear goal")?;
        Ok(done.rows_affected() > 0)
    }
}

fn decode(row: SqliteRow) -> Result<Goal> {
    let rounds: i64 = row.try_get("rounds")?;
    Ok(Goal {
        thread_id: row.try_get("thread_id")?,
        text: row.try_get("text")?,
        status: GoalStatus::parse(row.try_get("status")?)?,
        rounds: u32::try_from(rounds).context("goal rounds out of range")?,
        note: row.try_get("note")?,
        revision: row.try_get("revision")?,
        criteria: serde_json::from_str(row.try_get("criteria_json")?)
            .context("decode goal criteria")?,
        approved: row.try_get::<Option<i64>, _>("approved_at")?.is_some(),
        verification: row.try_get("verification")?,
        verification_note: row.try_get("verification_note")?,
        evidence: row
            .try_get::<Option<&str>, _>("verification_request")?
            .map(serde_json::from_str)
            .transpose()?
            .unwrap_or_default(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::temp_state;

    #[tokio::test]
    async fn draft_requires_matching_user_approval_and_locks_the_contract() {
        let state = temp_state("goal-contract").await;
        state.create_thread("thread", "test").await.unwrap();
        let draft = state.set_goal("thread", "do work").await.unwrap();
        assert_eq!(draft.status, GoalStatus::Paused);
        assert!(!draft.approved);
        assert!(
            state
                .set_goal_status("thread", GoalStatus::Active, None)
                .await
                .is_err()
        );
        assert!(
            state.approve_goal("thread", &draft.revision).await.is_err(),
            "empty criteria cannot pass"
        );
        let criteria = vec![GoalCriterion {
            id: "behavior".into(),
            description: "human checks behavior".into(),
            check: GoalCheck::UserConfirmation,
        }];
        let proposed = state
            .propose_goal("thread", &draft.revision, criteria.clone())
            .await
            .unwrap();
        assert_ne!(draft.revision, proposed.revision);
        assert!(
            state.approve_goal("thread", &draft.revision).await.is_err(),
            "old proposal token is invalid"
        );
        let approved = state
            .approve_goal("thread", &proposed.revision)
            .await
            .unwrap();
        assert!(approved.approved);
        assert!(
            state
                .propose_goal("thread", &approved.revision, criteria)
                .await
                .is_err()
        );
        let edited = state.set_goal("thread", "changed scope").await.unwrap();
        assert_ne!(approved.revision, edited.revision);
        assert!(!edited.approved);
        assert_eq!(edited.verification, "unverified");
    }

    #[tokio::test]
    async fn migration_preserves_legacy_history_without_assuming_verification() {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        for sql in [
            include_str!("../db/migrations/0001_threads.sql"),
            include_str!("../db/migrations/0002_messages.sql"),
            include_str!("../db/migrations/0003_turns.sql"),
            include_str!("../db/migrations/0012_goals.sql"),
            include_str!("../db/migrations/0013_background_jobs.sql"),
        ] {
            sqlx::raw_sql(sql).execute(&pool).await.unwrap();
        }
        sqlx::raw_sql("INSERT INTO threads (id, source, created_at) VALUES ('active', 'test', 0), ('done', 'test', 0);
            INSERT INTO goals VALUES ('active','legacy work','active',2,NULL,0), ('done','legacy history','done',1,'claimed result',0);")
            .execute(&pool).await.unwrap();
        sqlx::raw_sql(include_str!("../db/migrations/0016_goal_verification.sql"))
            .execute(&pool)
            .await
            .unwrap();
        let state = State { pool };
        let active = state.goal("active").await.unwrap().unwrap();
        assert_eq!(active.status, GoalStatus::Paused);
        assert!(!active.approved);
        let done = state.goal("done").await.unwrap().unwrap();
        assert_eq!(done.status, GoalStatus::Done);
        assert_eq!(done.note.as_deref(), Some("claimed result"));
        assert_eq!(done.verification, "unverified");
    }
}
