//! A thread's standing goal, kept across restarts.

use anyhow::{Context, Result, bail, ensure};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sqlx::{Row, sqlite::SqliteRow};

mod contract;
mod verification;

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
            include_str!("../../db/migrations/0001_threads.sql"),
            include_str!("../../db/migrations/0002_messages.sql"),
            include_str!("../../db/migrations/0003_turns.sql"),
            include_str!("../../db/migrations/0012_goals.sql"),
            include_str!("../../db/migrations/0013_background_jobs.sql"),
        ] {
            sqlx::raw_sql(sql).execute(&pool).await.unwrap();
        }
        sqlx::raw_sql("INSERT INTO threads (id, source, created_at) VALUES ('active', 'test', 0), ('done', 'test', 0);
            INSERT INTO goals VALUES ('active','legacy work','active',2,NULL,0), ('done','legacy history','done',1,'claimed result',0);")
            .execute(&pool).await.unwrap();
        sqlx::raw_sql(include_str!(
            "../../db/migrations/0016_goal_verification.sql"
        ))
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
