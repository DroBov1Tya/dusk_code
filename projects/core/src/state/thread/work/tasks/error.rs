//! Why the plan's store refused. Every refusal is one of these, so a caller can tell a
//! lead what to do next without reading text.

use std::fmt;

use super::plan::PlanStatus;

#[derive(Debug)]
pub enum PlanError {
    NoSuchThread(String),
    /// No task of this plan has the id; a task of another chat's plan is not found either.
    NoSuchTask(String),
    /// The end of an id that fits more than one task.
    Ambiguous(String),
    /// The state machine has no such move.
    Illegal {
        task: String,
        from: PlanStatus,
        to: PlanStatus,
    },
    /// The thread that asked is not the one the task was given to.
    NotOwner {
        task: String,
    },
    NotClaimable {
        task: String,
        why: Unclaimable,
    },
    /// The task's gate is not this caller's to pass.
    Gate {
        task: String,
        why: &'static str,
    },
    /// A blocked task that waits for a failed one opens when that one does.
    BehindFailed {
        task: String,
        dependency: String,
    },
    /// The task ids of a loop of dependencies, from a task back to itself.
    Cycle(Vec<String>),
    /// A dependency that is neither a key of the same `add` nor a task of the plan.
    UnknownDependency(String),
    DuplicateKey(String),
    Empty(&'static str),
    TooLong {
        what: &'static str,
        max: usize,
    },
    TooMany {
        what: &'static str,
        max: usize,
    },
    BadFile {
        entry: String,
        why: &'static str,
    },
    /// The turn is not one of the owner's.
    NoSuchTurn(String),
    Store(anyhow::Error),
}

/// Which condition of the claim did not hold.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unclaimable {
    /// A task it depends on is not done.
    Dependency(String),
    /// A task at work or in review holds an overlapping entry.
    Files { holder: String, entry: String },
    /// As many tasks are at work as the team runs at once.
    AtOnce(u32),
    /// The thread already has a task that is not finished.
    OwnerBusy,
    /// The thread is not under the plan's chat.
    ForeignOwner,
}

/// The last 8 characters of a task or step id, as the board, the plan and their errors
/// show it.
pub fn short_task_id(id: &str) -> &str {
    let from = id.char_indices().rev().nth(7).map_or(0, |(index, _)| index);
    &id[from..]
}

/// At most 80 characters of text that came from outside, for an error message.
pub(super) fn shown(text: &str) -> String {
    dusk_protocol::text::clip_chars(text, 80, "")
}

impl fmt::Display for PlanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoSuchThread(_) => write!(f, "no such thread"),
            Self::NoSuchTask(id) => write!(f, "no task {} in this plan", shown(id)),
            Self::Ambiguous(id) => {
                write!(f, "{} fits more than one task; give more of it", shown(id))
            }
            Self::Illegal { task, from, to } => write!(
                f,
                "task #{} is {}; it cannot become {}",
                short_task_id(task),
                from.as_str(),
                to.as_str()
            ),
            Self::NotOwner { task } => {
                write!(
                    f,
                    "task #{} was given to another thread",
                    short_task_id(task)
                )
            }
            Self::NotClaimable { task, why } => {
                write!(f, "task #{} cannot start: ", short_task_id(task))?;
                match why {
                    Unclaimable::Dependency(dependency) => {
                        write!(f, "#{} is not done", short_task_id(dependency))
                    }
                    Unclaimable::Files { holder, entry } => {
                        write!(f, "#{} holds {}", short_task_id(holder), shown(entry))
                    }
                    Unclaimable::AtOnce(limit) => write!(f, "{limit} are at work already"),
                    Unclaimable::OwnerBusy => write!(f, "the thread has an unfinished task"),
                    Unclaimable::ForeignOwner => write!(f, "the thread is not of this plan"),
                }
            }
            Self::Gate { task, why } => write!(f, "task #{}: {why}", short_task_id(task)),
            Self::BehindFailed { task, dependency } => write!(
                f,
                "task #{} waits for failed #{}; reopen that one",
                short_task_id(task),
                short_task_id(dependency)
            ),
            Self::Cycle(path) => match path.as_slice() {
                [] => write!(f, "the tasks depend on each other in a loop"),
                [only] | [only, _] => write!(f, "task #{} depends on itself", short_task_id(only)),
                [first, rest @ ..] => {
                    write!(f, "task #{}", short_task_id(first))?;
                    for (index, next) in rest.iter().enumerate() {
                        let link = if index == 0 {
                            " depends on"
                        } else {
                            ", which depends on"
                        };
                        write!(f, "{link} #{}", short_task_id(next))?;
                    }
                    Ok(())
                }
            },
            Self::UnknownDependency(id) => {
                write!(
                    f,
                    "{} is neither a key of this add nor a task of the plan",
                    shown(id)
                )
            }
            Self::DuplicateKey(key) => write!(f, "the key {} is used twice", shown(key)),
            Self::Empty(what) => write!(f, "{what} is empty"),
            Self::TooLong { what, max } => write!(f, "{what} is longer than {max} characters"),
            Self::TooMany { what, max } => write!(f, "more than {max} {what}"),
            Self::BadFile { entry, why } => write!(f, "{}: {why}", shown(entry)),
            Self::NoSuchTurn(_) => write!(f, "no such turn of the task's thread"),
            Self::Store(error) => write!(f, "the plan's store failed: {error:#}"),
        }
    }
}

impl std::error::Error for PlanError {}

impl From<sqlx::Error> for PlanError {
    fn from(error: sqlx::Error) -> Self {
        Self::Store(error.into())
    }
}

impl From<anyhow::Error> for PlanError {
    fn from(error: anyhow::Error) -> Self {
        Self::Store(error)
    }
}

impl From<serde_json::Error> for PlanError {
    fn from(error: serde_json::Error) -> Self {
        Self::Store(error.into())
    }
}
