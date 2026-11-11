//! A thread's standing goal, its acceptance criteria and their evidence.

use anyhow::{Result, bail, ensure};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GoalStatus {
    Active,
    Paused,
    Done,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
    pub fn validate(criteria: &[Self]) -> Result<()> {
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

    pub fn parse(text: &str) -> Result<Self> {
        Ok(match text {
            "active" => Self::Active,
            "paused" => Self::Paused,
            "done" => Self::Done,
            other => bail!("unknown goal status {other}"),
        })
    }
}
