//! The task board of a thread, and the plan a team runs: the tasks of a root thread, with
//! their dependencies, owners and gates.

use anyhow::{Result, bail, ensure};
use serde::{Deserialize, Serialize};

use crate::thread::GoalCheck;

/// The most done tasks one page of `WorkRequest::PlanTasks` holds.
pub const PLAN_PAGE_DONE: u32 = 100;

/// A task. Every field after `items` came with the plan graph and has a default, so a
/// task written before it still reads; such a task's status follows its `done`.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "TaskFields")]
pub struct Task {
    pub task_id: String,
    pub description: String,
    /// Always `status == TaskStatus::Done`; kept for clients that only know a board.
    pub done: bool,
    pub created_at: i64,
    pub updated_at: i64,
    /// In order.
    pub items: Vec<TaskItem>,
    pub status: TaskStatus,
    /// Ids of the tasks of the same plan that must be done first.
    pub depends_on: Vec<String>,
    /// The agent definition that does it; `None` is the general agent.
    pub role: Option<String>,
    /// A label that groups tasks on the strip.
    pub phase: String,
    pub acceptance: String,
    pub brief: String,
    /// What passes it from review; `None` waits for the lead.
    pub gate: Option<GoalCheck>,
    /// The paths only this task may change, relative to the project; a folder ends in `/`.
    pub files: Vec<String>,
    /// The member that works on it or did; `None` while it is open.
    pub owner: Option<TaskOwner>,
    /// Times it was given out.
    pub attempts: u32,
    /// Times it came back from review.
    pub reworks: u32,
    /// What a blocked task waits for.
    pub waits_on: Option<WaitsOn>,
    /// Why it is blocked or failed, or what the review said. Model or program text: data.
    pub note: String,
    /// The model it was given to, as `provider/model`.
    pub model: Option<String>,
    /// Model calls of its turns.
    pub steps: u32,
    /// Prompt and completion tokens of its turns.
    pub tokens: u64,
    /// Files its turns changed.
    pub changes: u32,
    /// The footer its member handed in with, or a reviewer's verdict. Model text: data.
    pub result: String,
    /// What the row is for: the lead's work, or a check the core added.
    pub duty: TaskDuty,
    /// For a review: the task it reviews.
    pub review_of: Option<String>,
    /// The lead gave it up: it failed or was blocked, and no longer holds the next wave.
    pub skipped: bool,
}

/// What a task of a plan is for.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskDuty {
    /// A task of the lead's plan.
    #[default]
    Work,
    /// The core's independent review of a task that changed files.
    Review,
    /// The core's check at the end: what the request asked that no task did or verified.
    Critic,
}

impl TaskDuty {
    pub fn name(self) -> &'static str {
        match self {
            Self::Work => "work",
            Self::Review => "review",
            Self::Critic => "critic",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        [Self::Work, Self::Review, Self::Critic]
            .into_iter()
            .find(|duty| duty.name() == text)
    }
}

/// The fields of a `Task` as they arrive. `status` is optional only so a task written
/// before the plan graph takes it from `done`.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Deserialize)]
struct TaskFields {
    task_id: String,
    description: String,
    done: bool,
    created_at: i64,
    updated_at: i64,
    items: Vec<TaskItem>,
    #[serde(default)]
    status: Option<TaskStatus>,
    #[serde(default)]
    depends_on: Vec<String>,
    #[serde(default)]
    role: Option<String>,
    #[serde(default)]
    phase: String,
    #[serde(default)]
    acceptance: String,
    #[serde(default)]
    brief: String,
    #[serde(default)]
    gate: Option<GoalCheck>,
    #[serde(default)]
    files: Vec<String>,
    #[serde(default)]
    owner: Option<TaskOwner>,
    #[serde(default)]
    attempts: u32,
    #[serde(default)]
    reworks: u32,
    #[serde(default)]
    waits_on: Option<WaitsOn>,
    #[serde(default)]
    note: String,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    steps: u32,
    #[serde(default)]
    tokens: u64,
    #[serde(default)]
    changes: u32,
    #[serde(default)]
    result: String,
    #[serde(default)]
    duty: TaskDuty,
    #[serde(default)]
    review_of: Option<String>,
    #[serde(default)]
    skipped: bool,
}

impl TryFrom<TaskFields> for Task {
    type Error = anyhow::Error;

    fn try_from(fields: TaskFields) -> Result<Self> {
        let status = fields.status.unwrap_or(TaskStatus::from_done(fields.done));
        ensure!(
            fields.done == (status == TaskStatus::Done),
            "task {} is {} but done is {}",
            fields.task_id,
            status.name(),
            fields.done
        );
        Ok(Self {
            task_id: fields.task_id,
            description: fields.description,
            done: fields.done,
            created_at: fields.created_at,
            updated_at: fields.updated_at,
            items: fields.items,
            status,
            depends_on: fields.depends_on,
            role: fields.role,
            phase: fields.phase,
            acceptance: fields.acceptance,
            brief: fields.brief,
            gate: fields.gate,
            files: fields.files,
            owner: fields.owner,
            attempts: fields.attempts,
            reworks: fields.reworks,
            waits_on: fields.waits_on,
            note: fields.note,
            model: fields.model,
            steps: fields.steps,
            tokens: fields.tokens,
            changes: fields.changes,
            result: fields.result,
            duty: fields.duty,
            review_of: fields.review_of,
            skipped: fields.skipped,
        })
    }
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskItem {
    pub item_id: String,
    pub description: String,
    pub done: bool,
    pub created_at: i64,
    pub updated_at: i64,
    /// `None` until work on it begins.
    pub started_at: Option<i64>,
    /// Set exactly when it is done.
    pub finished_at: Option<i64>,
}

/// Where a task is. Only the core moves a task from one to another.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    #[default]
    Open,
    /// Given to a member whose turn has not started.
    Claimed,
    Running,
    Blocked,
    /// Handed in; its gate or the lead decides.
    Review,
    /// Final.
    Done,
    Failed,
}

impl TaskStatus {
    /// The status of a task that only has a board's `done`.
    pub fn from_done(done: bool) -> Self {
        if done { Self::Done } else { Self::Open }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Claimed => "claimed",
            Self::Running => "running",
            Self::Blocked => "blocked",
            Self::Review => "review",
            Self::Done => "done",
            Self::Failed => "failed",
        }
    }

    /// How a team member's state reads beside its name: a claimed task still "waits".
    pub fn member_word(self) -> &'static str {
        match self {
            Self::Open | Self::Claimed => "waits",
            Self::Running => "running",
            Self::Blocked => "blocked",
            Self::Review => "in review",
            Self::Done => "done",
            Self::Failed => "failed",
        }
    }

    pub fn parse(text: &str) -> Result<Self> {
        Ok(match text {
            "open" => Self::Open,
            "claimed" => Self::Claimed,
            "running" => Self::Running,
            "blocked" => Self::Blocked,
            "review" => Self::Review,
            "done" => Self::Done,
            "failed" => Self::Failed,
            other => bail!("unknown task status {other}"),
        })
    }
}

/// The member thread that holds a task.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskOwner {
    pub thread_id: String,
    pub name: String,
}

/// What a blocked task waits for.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WaitsOn {
    /// A task of the plan that failed, by its id.
    Task(String),
    /// A message of the team's mailbox that nobody answered yet, by its id.
    Message(i64),
    /// A task in review that the core held for the lead's decision.
    Lead,
    /// A task in review that a reviewer row, by its id, checks now.
    Review(String),
}

/// One page of a plan, for the strip: every task not done, then done ones, newest first.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanPage {
    pub tasks: Vec<Task>,
    /// The `done_before` of the next page; `None` when no done task is left.
    pub next: Option<String>,
    /// The whole plan's tasks by status, on this page or not.
    pub counts: PlanCounts,
}

/// How many tasks of a plan are in each status.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanCounts {
    pub open: u32,
    pub claimed: u32,
    pub running: u32,
    pub blocked: u32,
    pub review: u32,
    pub done: u32,
    pub failed: u32,
}

impl PlanCounts {
    pub fn total(&self) -> u32 {
        [
            self.open,
            self.claimed,
            self.running,
            self.blocked,
            self.review,
            self.done,
            self.failed,
        ]
        .into_iter()
        .fold(0, u32::saturating_add)
    }

    /// Tasks a member holds: claimed or running.
    pub fn at_work(&self) -> u32 {
        self.claimed.saturating_add(self.running)
    }

    /// The progress bar's shares in its order: done, at work or in review, blocked or
    /// failed, open.
    pub fn shares(&self) -> [u32; 4] {
        [
            self.done,
            self.at_work().saturating_add(self.review),
            self.blocked.saturating_add(self.failed),
            self.open,
        ]
    }

    /// "3 of 7 done · 1 running · 1 blocked": the parts that are not zero after the done.
    pub fn summary(&self) -> String {
        let mut text = format!("{} of {} done", self.done, self.total());
        for (count, word) in [
            (self.running, "running"),
            (self.claimed, "claimed"),
            (self.review, "in review"),
            (self.blocked, "blocked"),
            (self.failed, "failed"),
        ] {
            if count > 0 {
                text.push_str(&format!(" \u{00B7} {count} {word}"));
            }
        }
        text
    }
}

/// The task a plan strip names, with its number from 1: the first running, else the
/// first that is not over.
pub fn current_task(tasks: &[Task]) -> Option<(usize, &Task)> {
    let numbered = || {
        tasks
            .iter()
            .enumerate()
            .map(|(index, task)| (index + 1, task))
    };
    numbered()
        .find(|(_, task)| task.status == TaskStatus::Running)
        .or_else(|| {
            numbered()
                .find(|(_, task)| !matches!(task.status, TaskStatus::Done | TaskStatus::Failed))
        })
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;

    fn board_task(done: bool) -> Value {
        json!({
            "task_id": "t1",
            "description": "write it",
            "done": done,
            "created_at": 1,
            "updated_at": 2,
            "items": [],
        })
    }

    fn read(value: Value) -> Result<Task, String> {
        serde_json::from_value(value).map_err(|error| error.to_string())
    }

    #[test]
    fn a_task_from_before_the_plan_graph_takes_its_status_from_done() {
        for done in [false, true] {
            let task = read(board_task(done)).unwrap();
            assert_eq!(task.status, TaskStatus::from_done(done));
            assert_eq!(task.done, done);
            assert!(task.depends_on.is_empty() && task.owner.is_none() && task.gate.is_none());
        }
    }

    #[test]
    fn done_and_status_must_agree() {
        for (done, status, fits) in [
            (true, "done", true),
            (false, "review", true),
            (true, "review", false),
            (false, "done", false),
        ] {
            let mut value = board_task(done);
            value["status"] = json!(status);
            assert_eq!(read(value).is_ok(), fits, "{done} {status}");
        }
    }

    #[test]
    fn a_plan_page_survives_a_round_trip() {
        let mut value = board_task(false);
        value["status"] = json!("blocked");
        value["depends_on"] = json!(["t0"]);
        value["role"] = json!("reviewer");
        value["gate"] = json!({ "kind": "command", "command": "cargo test" });
        value["files"] = json!(["src/", "Cargo.toml"]);
        value["owner"] = json!({ "thread_id": "m1", "name": "Ada" });
        value["waits_on"] = json!({ "task": "t0" });
        value["tokens"] = json!(12_345);
        let task = read(value).unwrap();
        assert_eq!(task.waits_on, Some(WaitsOn::Task("t0".into())));
        let page = PlanPage {
            tasks: vec![task],
            next: Some("t0".into()),
            counts: PlanCounts {
                blocked: 1,
                ..PlanCounts::default()
            },
        };
        let text = serde_json::to_string(&page).unwrap();
        assert_eq!(serde_json::from_str::<PlanPage>(&text).unwrap(), page);
    }

    #[test]
    fn every_wait_survives_a_round_trip() {
        for waits in [
            WaitsOn::Task("t0".into()),
            WaitsOn::Message(7),
            WaitsOn::Lead,
        ] {
            let text = serde_json::to_string(&waits).unwrap();
            assert_eq!(serde_json::from_str::<WaitsOn>(&text).unwrap(), waits);
        }
    }

    #[test]
    fn every_status_reads_back_by_its_name() {
        for status in [
            TaskStatus::Open,
            TaskStatus::Claimed,
            TaskStatus::Running,
            TaskStatus::Blocked,
            TaskStatus::Review,
            TaskStatus::Done,
            TaskStatus::Failed,
        ] {
            assert_eq!(TaskStatus::parse(status.name()).unwrap(), status);
            assert_eq!(serde_json::to_value(status).unwrap(), json!(status.name()));
        }
        assert!(TaskStatus::parse("paused").is_err());
        assert!(serde_json::from_value::<TaskStatus>(json!("paused")).is_err());
    }

    #[test]
    fn plan_counts_sum_saturate_and_name_only_what_is_there() {
        let counts = PlanCounts {
            open: 1,
            claimed: 2,
            running: 3,
            blocked: 4,
            review: 5,
            done: 6,
            failed: 7,
        };
        assert_eq!(counts.total(), 28);
        assert_eq!(counts.at_work(), 5);
        assert_eq!(counts.shares(), [6, 10, 11, 1]);
        assert_eq!(
            counts.summary(),
            "6 of 28 done \u{00B7} 3 running \u{00B7} 2 claimed \u{00B7} 5 in review \
             \u{00B7} 4 blocked \u{00B7} 7 failed"
        );
        let full = PlanCounts {
            open: u32::MAX,
            done: 1,
            claimed: u32::MAX,
            ..PlanCounts::default()
        };
        assert_eq!(full.total(), u32::MAX);
        assert_eq!(full.at_work(), u32::MAX);
        assert_eq!(PlanCounts::default().total(), 0);
        assert_eq!(PlanCounts::default().summary(), "0 of 0 done");
        let some = PlanCounts {
            done: 2,
            blocked: 1,
            ..PlanCounts::default()
        };
        assert_eq!(some.summary(), "2 of 3 done \u{00B7} 1 blocked");
    }

    #[test]
    fn the_current_task_is_the_first_running_else_the_first_not_over() {
        let task = |id: &str, status: TaskStatus| Task {
            task_id: id.into(),
            status,
            ..Task::default()
        };
        let number = |tasks: &[Task]| current_task(tasks).map(|(n, t)| (n, t.task_id.clone()));
        assert_eq!(number(&[]), None);
        let over = [task("a", TaskStatus::Done), task("b", TaskStatus::Failed)];
        assert_eq!(number(&over), None);
        let waiting = [
            task("a", TaskStatus::Done),
            task("b", TaskStatus::Blocked),
            task("c", TaskStatus::Open),
        ];
        assert_eq!(number(&waiting), Some((2, "b".into())));
        let running = [
            task("a", TaskStatus::Open),
            task("b", TaskStatus::Running),
            task("c", TaskStatus::Running),
        ];
        assert_eq!(number(&running), Some((2, "b".into())));
    }

    #[test]
    fn a_member_word_says_a_claimed_task_still_waits() {
        let words: Vec<&str> = [
            TaskStatus::Open,
            TaskStatus::Claimed,
            TaskStatus::Running,
            TaskStatus::Blocked,
            TaskStatus::Review,
            TaskStatus::Done,
            TaskStatus::Failed,
        ]
        .into_iter()
        .map(TaskStatus::member_word)
        .collect();
        assert_eq!(
            words,
            [
                "waits",
                "waits",
                "running",
                "blocked",
                "in review",
                "done",
                "failed"
            ]
        );
    }
}
