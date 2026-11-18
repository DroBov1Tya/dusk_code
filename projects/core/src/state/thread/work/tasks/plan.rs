//! The words of a plan: a task's status, its row as the store keeps it, what a lead
//! writes into one, and the caps.

use dusk_protocol::thread::GoalCheck;
use sqlx::{Row, sqlite::SqliteRow};

use super::{error::PlanError, files::TaskFiles};

/// Tasks not done in one plan.
pub const MAX_OPEN_TASKS: usize = 64;
/// Tasks one `add` makes.
pub const MAX_TASKS_AN_ADD: usize = 32;
/// Times a task is given out before a release fails it.
pub const MAX_ATTEMPTS: u32 = 3;
/// Times a task comes back from review before the next rejection fails it.
pub const MAX_REWORKS: u32 = 2;
/// Entries in a task's `files`.
pub const MAX_FILES: usize = 16;
/// How long a claim holds without a renewal.
pub const LEASE_MS: i64 = 120_000;
/// Tasks one task depends on. Not in the design: a bound for the walk of the graph.
pub const MAX_DEPENDENCIES: usize = 32;

// In characters, as the CHECKs of migration 0036 count them. The description has no
// CHECK (the column is older); its cap is the store's own.
pub const MAX_DESCRIPTION: usize = 600;
pub const MAX_ROLE: usize = 128;
pub const MAX_PHASE: usize = 32;
pub const MAX_ACCEPTANCE: usize = 600;
pub const MAX_BRIEF: usize = 1200;
pub const MAX_NOTE: usize = 600;
pub const MAX_KEY: usize = 64;
pub const MAX_MODEL: usize = 200;

/// The one status of a task; the names are the CHECK list of migration 0036.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PlanStatus {
    Open,
    Claimed,
    Running,
    Blocked,
    Review,
    Done,
    Failed,
}

impl PlanStatus {
    pub const ALL: [Self; 7] = [
        Self::Open,
        Self::Claimed,
        Self::Running,
        Self::Blocked,
        Self::Review,
        Self::Done,
        Self::Failed,
    ];

    pub fn as_str(self) -> &'static str {
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

    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|status| status.as_str() == text)
    }
}

/// What a member that cannot go on waits for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlockedOn {
    /// A task of the same plan.
    Task(String),
    /// A row of `team_messages`.
    Message(i64),
}

/// `tasks.waits_on` as read back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WaitsOn {
    Task(String),
    Message(i64),
    /// A task in review that the core held for the lead whatever its gate says.
    Lead,
}

impl WaitsOn {
    pub(super) fn stored(&self) -> String {
        match self {
            Self::Task(id) => format!("task:{id}"),
            Self::Message(id) => format!("message:{id}"),
            Self::Lead => "lead".to_owned(),
        }
    }

    fn parse(text: &str) -> Option<Self> {
        if text == "lead" {
            return Some(Self::Lead);
        }
        if let Some(id) = text.strip_prefix("task:") {
            return Some(Self::Task(id.to_owned()));
        }
        text.strip_prefix("message:")?
            .parse()
            .ok()
            .map(Self::Message)
    }
}

/// A task of a plan, every column of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanTask {
    pub task_id: String,
    /// The root thread the plan belongs to.
    pub plan: String,
    pub description: String,
    pub status: PlanStatus,
    pub depends_on: Vec<String>,
    pub role: Option<String>,
    pub phase: String,
    pub acceptance: String,
    pub brief: String,
    pub gate: Option<GoalCheck>,
    pub files: TaskFiles,
    pub owner_thread_id: Option<String>,
    pub lease_until: Option<i64>,
    pub attempts: u32,
    pub reworks: u32,
    pub waits_on: Option<WaitsOn>,
    pub note: String,
    pub gate_job: Option<String>,
    pub checkpoint: Option<String>,
    pub model: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

/// A task as a lead writes it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NewTask {
    /// The lead's label for it, which other tasks of the same `add` name in `after`.
    pub key: Option<String>,
    pub description: String,
    /// What must be done first: keys of this `add`, or ids of tasks of the plan (or the
    /// ends of them). A key wins over an id that ends the same.
    pub after: Vec<String>,
    pub role: Option<String>,
    pub phase: String,
    pub acceptance: String,
    pub brief: String,
    pub gate: Option<GoalCheck>,
    pub files: TaskFiles,
}

/// What an `edit` changes; `None` leaves a field as it is.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TaskEdit {
    pub description: Option<String>,
    /// Ids of tasks of the plan, or the ends of them.
    pub after: Option<Vec<String>>,
    pub role: Option<Option<String>>,
    pub phase: Option<String>,
    pub acceptance: Option<String>,
    pub brief: Option<String>,
    pub gate: Option<Option<GoalCheck>>,
    pub files: Option<TaskFiles>,
}

/// The columns `decode` reads, in front of the rest of a query.
macro_rules! select_tasks {
    ($rest:literal) => {
        concat!(
            "SELECT t.task_id, t.thread_id, t.description, t.status, t.depends_on, t.role, \
             t.phase, t.acceptance, t.brief, t.gate, t.files, t.owner_thread_id, \
             t.lease_until, t.attempts, t.reworks, t.waits_on, t.note, t.gate_job, \
             t.checkpoint, t.model, t.created_at, t.updated_at FROM tasks t ",
            $rest
        )
    };
}
pub(super) use select_tasks;

pub(super) fn decode(row: &SqliteRow) -> Result<PlanTask, PlanError> {
    let status: String = row.try_get("status")?;
    let status = PlanStatus::parse(&status)
        .ok_or_else(|| PlanError::Store(anyhow::anyhow!("unknown task status {status}")))?;
    let depends_on: String = row.try_get("depends_on")?;
    let files: String = row.try_get("files")?;
    let files: Vec<String> = serde_json::from_str(&files)?;
    let gate: Option<String> = row.try_get("gate")?;
    let waits_on: Option<String> = row.try_get("waits_on")?;
    let count = |name: &str| -> Result<u32, PlanError> {
        let value: i64 = row.try_get(name)?;
        u32::try_from(value).map_err(|error| PlanError::Store(error.into()))
    };
    Ok(PlanTask {
        task_id: row.try_get("task_id")?,
        plan: row.try_get("thread_id")?,
        description: row.try_get("description")?,
        status,
        depends_on: serde_json::from_str(&depends_on)?,
        role: row.try_get("role")?,
        phase: row.try_get("phase")?,
        acceptance: row.try_get("acceptance")?,
        brief: row.try_get("brief")?,
        gate: gate.as_deref().map(serde_json::from_str).transpose()?,
        files: TaskFiles::new(files)?,
        owner_thread_id: row.try_get("owner_thread_id")?,
        lease_until: row.try_get("lease_until")?,
        attempts: count("attempts")?,
        reworks: count("reworks")?,
        waits_on: waits_on.as_deref().and_then(WaitsOn::parse),
        note: row.try_get("note")?,
        gate_job: row.try_get("gate_job")?,
        checkpoint: row.try_get("checkpoint")?,
        model: row.try_get("model")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

/// Refuses text over its cap; the caller cuts, the store never does it silently.
pub(super) fn capped(what: &'static str, text: &str, max: usize) -> Result<(), PlanError> {
    // Bytes first: a text of `max` bytes or fewer cannot have more characters.
    if text.len() > max && text.chars().count() > max {
        return Err(PlanError::TooLong { what, max });
    }
    Ok(())
}

/// The checks of a task's texts that `add` and `edit` share.
pub(super) fn check_texts(
    description: Option<&str>,
    role: Option<&str>,
    phase: Option<&str>,
    acceptance: Option<&str>,
    brief: Option<&str>,
) -> Result<(), PlanError> {
    if let Some(description) = description {
        if description.trim().is_empty() {
            return Err(PlanError::Empty("the task's text"));
        }
        capped("the task's text", description.trim(), MAX_DESCRIPTION)?;
    }
    if let Some(role) = role {
        if role.is_empty() {
            return Err(PlanError::Empty("the role"));
        }
        capped("the role", role, MAX_ROLE)?;
    }
    capped("the phase", phase.unwrap_or_default(), MAX_PHASE)?;
    capped(
        "the acceptance",
        acceptance.unwrap_or_default(),
        MAX_ACCEPTANCE,
    )?;
    capped("the brief", brief.unwrap_or_default(), MAX_BRIEF)
}
