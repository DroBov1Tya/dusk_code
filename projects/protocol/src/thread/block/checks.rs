use anyhow::ensure;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::Line;

pub const MAX_CHECKS: usize = 20;
pub const CHECK_NAME_CHARS: usize = 80;
pub const CHECK_DETAIL_CHARS: usize = 120;

// Plain comments on the types here: a doc comment would go into the schema of `show`
// and so into every request.

// Checks such as test runs and CI jobs, each with the state it is in. Built and read
// only through `new`, so a list that exists is within its caps.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(try_from = "ChecksFields")]
pub struct Checks {
    items: Vec<Check>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ChecksFields {
    #[schemars(length(min = 1, max = MAX_CHECKS))]
    items: Vec<Check>,
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

// How many cases of a check passed and failed, of `total`; never more than the total.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(try_from = "CountsFields")]
pub struct Counts {
    passed: u32,
    failed: u32,
    total: u32,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct CountsFields {
    passed: u32,
    failed: u32,
    total: u32,
}

impl Counts {
    pub fn new(passed: u32, failed: u32, total: u32) -> anyhow::Result<Self> {
        ensure!(
            u64::from(passed) + u64::from(failed) <= u64::from(total),
            "counts of {passed} passed and {failed} failed are more than the total {total}"
        );
        Ok(Self {
            passed,
            failed,
            total,
        })
    }

    pub fn passed(self) -> u32 {
        self.passed
    }

    pub fn failed(self) -> u32 {
        self.failed
    }

    pub fn total(self) -> u32 {
        self.total
    }
}

impl Checks {
    pub fn new(items: Vec<Check>) -> anyhow::Result<Self> {
        ensure!(
            (1..=MAX_CHECKS).contains(&items.len()),
            "give 1 to {MAX_CHECKS} checks"
        );
        Ok(Self { items })
    }

    pub fn items(&self) -> &[Check] {
        &self.items
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

impl TryFrom<ChecksFields> for Checks {
    type Error = anyhow::Error;

    fn try_from(fields: ChecksFields) -> anyhow::Result<Self> {
        Self::new(fields.items)
    }
}

impl TryFrom<CountsFields> for Counts {
    type Error = anyhow::Error;

    fn try_from(fields: CountsFields) -> anyhow::Result<Self> {
        Self::new(fields.passed, fields.failed, fields.total)
    }
}
