//! A team: the core's run of the plan of a root thread, its budget and its members, and
//! the words every client shows for it and its plan.

mod budget;
mod members;
mod plan;
mod status;

pub use budget::BUDGET_FIELDS;
pub use members::{MemberRow, short_model};
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

/// From this share of a budget part on, clients draw it as nearly spent.
pub const BUDGET_NEAR: f32 = 0.8;

/// One part of a team's budget as the Overview draws it: its name, a bar and "used of all".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BudgetPart {
    pub name: &'static str,
    pub used: u64,
    pub all: u64,
    /// Counted in minutes; the others are plain counts.
    pub minutes: bool,
}

impl BudgetPart {
    /// How much is used, from 0 to 1.
    pub fn share(&self) -> f32 {
        (self.used as f32 / self.all.max(1) as f32).min(1.)
    }

    pub fn near(&self) -> bool {
        self.share() >= BUDGET_NEAR
    }

    /// "1.2k of 5k", or "12 of 60 min".
    pub fn text(&self) -> String {
        if self.minutes {
            format!("{} of {} min", self.used, self.all)
        } else {
            let short = crate::thread::short_count;
            format!("{} of {}", short(self.used), short(self.all))
        }
    }
}

impl Team {
    pub fn state_word(&self) -> &'static str {
        match self.status {
            TeamStatus::Running => "running",
            TeamStatus::Ended(_) => "ended",
        }
    }

    /// "running · 23 min", "done after 57 min", "stopped after 4 min", or "ended — ran
    /// out of steps after 57 min": how the team stands and, once it ended, why.
    pub fn progress_note(&self) -> String {
        let minutes = self.used.minutes;
        match self.ended() {
            None => format!("{} \u{00B7} {minutes} min", self.state_word()),
            Some(TeamEnd::Done) => format!("done after {minutes} min"),
            Some(TeamEnd::Stopped) => format!("stopped after {minutes} min"),
            Some(end) => format!("ended \u{2014} {} after {minutes} min", end.reason()),
        }
    }

    /// Members whose task is claimed or running.
    pub fn at_work(&self) -> usize {
        self.members
            .iter()
            .filter(|member| matches!(member.status, TaskStatus::Claimed | TaskStatus::Running))
            .count()
    }

    /// Steps, tokens and time: what is used of each part of the budget.
    pub fn budget_parts(&self) -> [BudgetPart; 3] {
        let (budget, used) = (self.budget, self.used);
        [
            BudgetPart {
                name: "Steps",
                used: used.steps.into(),
                all: budget.steps().into(),
                minutes: false,
            },
            BudgetPart {
                name: "Tokens",
                used: used.tokens,
                all: budget.tokens(),
                minutes: false,
            },
            BudgetPart {
                name: "Time",
                used: used.minutes.into(),
                all: budget.minutes().into(),
                minutes: true,
            },
        ]
    }
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

    fn team(status: TeamStatus, members: &[TaskStatus]) -> Team {
        Team {
            thread_id: "t".into(),
            status,
            budget: TeamBudget::new(2, 100, 5_000, 60).unwrap(),
            used: TeamUsed {
                at_once: 1,
                steps: 80,
                tokens: 1_250,
                minutes: 12,
            },
            members: members
                .iter()
                .map(|status| TeamMember {
                    thread_id: "m".into(),
                    name: "Ember".into(),
                    role: None,
                    model: None,
                    task_id: "x".into(),
                    status: *status,
                })
                .collect(),
        }
    }

    #[test]
    fn a_team_says_its_state_minutes_and_members_at_work() {
        let members = [
            TaskStatus::Open,
            TaskStatus::Claimed,
            TaskStatus::Running,
            TaskStatus::Review,
        ];
        let running = team(TeamStatus::Running, &members);
        assert_eq!(running.state_word(), "running");
        assert_eq!(running.progress_note(), "running \u{00B7} 12 min");
        assert_eq!(running.at_work(), 2);
        let ended = team(TeamStatus::Ended(TeamEnd::Done), &[]);
        // An ended team says why (see `status`).
        assert_eq!(ended.progress_note(), "done after 12 min");
        assert_eq!(ended.at_work(), 0);
        let texts: Vec<String> = running
            .budget_parts()
            .iter()
            .map(BudgetPart::text)
            .collect();
        assert_eq!(texts, ["80 of 100", "1.3k of 5k", "12 of 60 min"]);
        let names = running.budget_parts().map(|part| part.name);
        assert_eq!(names, ["Steps", "Tokens", "Time"]);
    }

    #[test]
    fn a_budget_part_is_near_from_eighty_percent_and_never_past_full() {
        let part = |used, all| BudgetPart {
            name: "Steps",
            used,
            all,
            minutes: false,
        };
        assert!(!part(79, 100).near());
        assert!(part(80, 100).near(), "exactly 80% is near");
        assert_eq!(part(80, 100).share(), 0.8);
        assert_eq!(part(150, 100).share(), 1.);
        assert_eq!(part(0, 0).share(), 0.);
        assert_eq!(part(5, 0).share(), 1., "nothing to spend is spent");
        assert_eq!(part(1_500, 2_000).text(), "1.5k of 2k");
    }
}
