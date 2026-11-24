//! A team: the core's run of the plan of a root thread, how many members work at once and
//! its members, and
//! the words every client shows for it and its plan.

mod card;
mod members;
mod plan;
mod roster;
mod status;

pub use card::{TaskCard, task_card};
pub use members::{MemberRow, short_model};
pub use plan::find_task;
pub use roster::{GENERAL_ROLE, Joining, KnownAgent, TeamChange, role_name};
pub use status::{plan_aside, task_in_work};

use serde::{Deserialize, Serialize};

use crate::thread::TaskStatus;

/// The most members one `Team` lists.
pub const MAX_TEAM_MEMBERS: usize = 64;
/// The most members any setting lets work at once; the core holds a team to its own, lower one.
pub const MAX_TEAM_AT_ONCE: u32 = 8;

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
    /// Members at work at once, 1 to the setting's `team_at_once`.
    pub at_once: u32,
    /// What it has used; shown, never a limit.
    pub used: TeamUsed,
    /// Newest first, at most `MAX_TEAM_MEMBERS`.
    pub members: Vec<TeamMember>,
    /// When this run of the team started, in Unix milliseconds. Running the plan again
    /// after a team ended starts a new run of the same chat's team; none from cores
    /// before runs were told apart.
    #[serde(default)]
    pub started_at: Option<i64>,
    /// When this run ended, in Unix milliseconds; none while it runs, and from cores
    /// before it was told.
    #[serde(default)]
    pub ended_at: Option<i64>,
    /// What the lead ran this team toward: one sentence and how done is told. Empty from
    /// cores before it was asked for.
    #[serde(default)]
    pub goal: String,
    /// Since when a running team winds down, in Unix milliseconds: it gives out nothing
    /// new and waits for its members' hand-overs before it ends. None from older cores.
    #[serde(default)]
    pub winding_since: Option<i64>,
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
    /// Older cores only: their team ran out of a budget part (steps, tokens or time).
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

/// What a team has used, counted by the core and shown as is: nothing bounds it.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TeamUsed {
    pub at_once: u32,
    pub steps: u32,
    pub tokens: u64,
    pub minutes: u32,
}

impl Team {
    pub fn state_word(&self) -> &'static str {
        match self.status {
            TeamStatus::Running if self.winding_since.is_some() => "wrapping up",
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

    /// Members whose task is claimed or running; none once the team ended, whatever a
    /// task's last status says.
    pub fn at_work(&self) -> usize {
        if self.ended().is_some() {
            return 0;
        }
        self.members
            .iter()
            .filter(|member| matches!(member.status, TaskStatus::Claimed | TaskStatus::Running))
            .count()
    }

    /// "41 steps \u{00B7} 512k tokens \u{00B7} 9 min": what the team has used so far.
    pub fn used_line(&self) -> String {
        let used = self.used;
        let short = crate::thread::short_count;
        format!(
            "{} steps \u{00B7} {} tokens \u{00B7} {} min",
            short(used.steps.into()),
            short(used.tokens),
            used.minutes
        )
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
    /// What it does, in a short line the core writes from its task: the start of the
    /// task's text, `review of #77aa01bc` or `completeness check`. Model text, made
    /// plain. Shown where the plan does not hold its task. Empty from older cores.
    #[serde(default)]
    pub summary: String,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn a_team_survives_a_round_trip() {
        for status in [TeamStatus::Running, TeamStatus::Ended(TeamEnd::Stalled)] {
            let team = Team {
                thread_id: "lead".into(),
                status,
                at_once: 3,
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
                    summary: String::new(),
                    status: TaskStatus::Review,
                }],
                started_at: None,
                ended_at: None,
                winding_since: None,
                goal: String::new(),
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
            at_once: 2,
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
                    summary: String::new(),
                    status: *status,
                })
                .collect(),
            started_at: None,
            ended_at: None,
            winding_since: None,
            goal: String::new(),
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
        assert_eq!(
            running.used_line(),
            "80 steps \u{00B7} 1.3k tokens \u{00B7} 12 min"
        );
    }
}
