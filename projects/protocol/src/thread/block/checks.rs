use anyhow::ensure;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::Line;

pub const MAX_CHECKS: usize = 20;
pub const CHECK_NAME_CHARS: usize = 80;
pub const CHECK_DETAIL_CHARS: usize = 120;

// Plain comments on the types here: a doc comment would go into the schema of `show`
// and so into every request.

// Checks such as test runs and CI jobs, each with the state it is in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Checks {
    #[schemars(length(min = 1, max = MAX_CHECKS))]
    pub items: Vec<Check>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Check {
    pub name: Line<CHECK_NAME_CHARS>,
    pub state: CheckState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub counts: Option<Counts>,
    /// One short line, such as the first failure.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<Line<CHECK_DETAIL_CHARS>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CheckState {
    Queued,
    Running,
    Passed,
    Failed,
    Skipped,
}

impl CheckState {
    pub fn name(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Passed => "passed",
            Self::Failed => "failed",
            Self::Skipped => "skipped",
        }
    }
}

// How many cases of a check passed and failed, of `total`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Counts {
    pub passed: u32,
    pub failed: u32,
    pub total: u32,
}

impl Checks {
    pub(super) fn check(&self) -> anyhow::Result<()> {
        ensure!(
            (1..=MAX_CHECKS).contains(&self.items.len()),
            "give 1 to {MAX_CHECKS} checks"
        );
        for check in &self.items {
            if let Some(counts) = check.counts {
                ensure!(
                    u64::from(counts.passed) + u64::from(counts.failed) <= u64::from(counts.total),
                    "the counts of {:?} add up to more than its total",
                    check.name.as_str()
                );
            }
        }
        Ok(())
    }

    pub(super) fn write(&self, text: &mut String) {
        for check in &self.items {
            text.push_str(&format!("{}: {}", check.state.name(), check.name));
            if let Some(counts) = check.counts {
                text.push_str(&format!(
                    " ({} passed, {} failed, {} total)",
                    counts.passed, counts.failed, counts.total
                ));
            }
            if let Some(detail) = &check.detail {
                text.push_str(&format!(" - {detail}"));
            }
            text.push('\n');
        }
    }
}
