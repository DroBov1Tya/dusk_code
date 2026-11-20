//! A team: the core's run of the plan of a root thread, its budget and its members, and
//! the words every client shows for it and its plan.

mod budget;
mod plan;
mod status;

pub use budget::BUDGET_FIELDS;
pub use plan::find_task;
pub use status::task_in_work;

use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

use crate::thread::TaskStatus;

/// The most members one `Team` lists.
pub const MAX_TEAM_MEMBERS: usize = 64;
/// The largest budget any ceiling allows; the core holds a budget to its own, lower ones.
pub const MAX_TEAM_AT_ONCE: u32 = 8;
pub const MAX_TEAM_STEPS: u32 = 5_000;
pub const MAX_TEAM_TOKENS: u64 = 100_000_000;
pub const MAX_TEAM_MINUTES: u32 = 1_440;

/// The `call_id` of the `SubagentStarted` that announces the member given `task_id`.
pub fn team_call_id(task_id: &str) -> String {
    format!("team:{task_id}")
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Team {
    /// The root thread, which leads it.
    pub thread_id: String,
    pub status: TeamStatus,
    pub budget: TeamBudget,
    pub used: TeamUsed,
    /// Newest first, at most `MAX_TEAM_MEMBERS`.
    pub members: Vec<TeamMember>,
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TeamStatus {
    Running,
    Ended(TeamEnd),
}

/// Why a team ended.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TeamEnd {
    /// Every task is done.
    Done,
    Steps,
    Tokens,
    Time,
    /// Nothing could start, and the lead changed nothing.
    Stalled,
    /// The lead or the user stopped it.
    Stopped,
    /// The lead reached the cap of agent messages.
    Quiet,
}

/// How much a team may spend unasked. Built and read only through `new`, so a budget
/// that exists is above zero and within the largest ceilings.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "TeamBudgetFields")]
pub struct TeamBudget {
    at_once: u32,
    steps: u32,
    tokens: u64,
    minutes: u32,
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TeamBudgetFields {
    at_once: u32,
    steps: u32,
    tokens: u64,
    minutes: u32,
}

impl TryFrom<TeamBudgetFields> for TeamBudget {
    type Error = anyhow::Error;

    fn try_from(fields: TeamBudgetFields) -> Result<Self> {
        Self::new(fields.at_once, fields.steps, fields.tokens, fields.minutes)
    }
}

impl TeamBudget {
    pub fn new(at_once: u32, steps: u32, tokens: u64, minutes: u32) -> Result<Self> {
        ensure!(
            (1..=MAX_TEAM_AT_ONCE).contains(&at_once),
            "members at once must be 1 to {MAX_TEAM_AT_ONCE}, not {at_once}"
        );
        ensure!(
            (1..=MAX_TEAM_STEPS).contains(&steps),
            "steps must be 1 to {MAX_TEAM_STEPS}, not {steps}"
        );
        ensure!(
            (1..=MAX_TEAM_TOKENS).contains(&tokens),
            "tokens must be 1 to {MAX_TEAM_TOKENS}, not {tokens}"
        );
        ensure!(
            (1..=MAX_TEAM_MINUTES).contains(&minutes),
            "minutes must be 1 to {MAX_TEAM_MINUTES}, not {minutes}"
        );
        Ok(Self {
            at_once,
            steps,
            tokens,
            minutes,
        })
    }

    /// Members at work at once.
    pub fn at_once(self) -> u32 {
        self.at_once
    }

    /// Model calls that answer, in the lead's whole tree.
    pub fn steps(self) -> u32 {
        self.steps
    }

    /// Prompt and completion tokens of every model call of the tree.
    pub fn tokens(self) -> u64 {
        self.tokens
    }

    /// Minutes from the start after which nothing is given out.
    pub fn minutes(self) -> u32 {
        self.minutes
    }
}

/// What a team has used of its budget, counted by the core; it may pass the budget by
/// the steps already under way.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TeamUsed {
    pub at_once: u32,
    pub steps: u32,
    pub tokens: u64,
    pub minutes: u32,
}

/// A thread that holds a task of the plan.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TeamMember {
    pub thread_id: String,
    pub name: String,
    /// Its agent definition; `None` is the general agent.
    pub role: Option<String>,
    /// As `provider/model`.
    pub model: Option<String>,
    pub task_id: String,
    /// The status of its task.
    pub status: TaskStatus,
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;

    fn budget(at_once: u32, steps: u32, tokens: u64, minutes: u32) -> Value {
        json!({ "at_once": at_once, "steps": steps, "tokens": tokens, "minutes": minutes })
    }

    fn fits(value: Value) -> bool {
        serde_json::from_value::<TeamBudget>(value).is_ok()
    }

    #[test]
    fn a_budget_is_taken_up_to_the_largest_ceilings_and_refused_past_them() {
        let (at_once, steps, tokens, minutes) = (
            MAX_TEAM_AT_ONCE,
            MAX_TEAM_STEPS,
            MAX_TEAM_TOKENS,
            MAX_TEAM_MINUTES,
        );
        assert!(fits(budget(1, 1, 1, 1)));
        assert!(fits(budget(at_once, steps, tokens, minutes)));
        for value in [
            budget(0, steps, tokens, minutes),
            budget(at_once + 1, steps, tokens, minutes),
            budget(at_once, 0, tokens, minutes),
            budget(at_once, steps + 1, tokens, minutes),
            budget(at_once, steps, 0, minutes),
            budget(at_once, steps, tokens + 1, minutes),
            budget(at_once, steps, tokens, 0),
            budget(at_once, steps, tokens, minutes + 1),
        ] {
            assert!(!fits(value.clone()), "{value}");
        }
        let mut extra = budget(1, 1, 1, 1);
        extra["dollars"] = json!(5);
        assert!(!fits(extra));
    }

    #[test]
    fn a_team_survives_a_round_trip() {
        for status in [TeamStatus::Running, TeamStatus::Ended(TeamEnd::Stalled)] {
            let team = Team {
                thread_id: "lead".into(),
                status,
                budget: TeamBudget::new(3, 400, 4_000_000, 120).unwrap(),
                used: TeamUsed {
                    at_once: 2,
                    steps: 41,
                    tokens: 512_000,
                    minutes: 9,
                },
                members: vec![TeamMember {
                    thread_id: "m1".into(),
                    name: "Ada".into(),
                    role: Some("reviewer".into()),
                    model: None,
                    task_id: "t1".into(),
                    status: TaskStatus::Review,
                }],
            };
            let text = serde_json::to_string(&team).unwrap();
            assert_eq!(serde_json::from_str::<Team>(&text).unwrap(), team);
        }
        assert_eq!(
            serde_json::to_value(TeamStatus::Ended(TeamEnd::Quiet)).unwrap(),
            json!({ "ended": "quiet" })
        );
    }
}
