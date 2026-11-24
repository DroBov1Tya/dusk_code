//! Why the team's store refused. Every refusal is one of these, so the runtime and the
//! serving layer can answer without reading text.

use std::fmt;

use dusk_protocol::thread::TeamEnd;

use super::super::PlanError;

#[derive(Debug)]
pub enum TeamError {
    NoSuchThread(String),
    /// No team has run in the chat.
    NoTeam,
    /// A team runs already; another run starts after its end.
    Running,
    /// The team is not running; it ended for this reason.
    Ended(TeamEnd),
    /// Tasks of the last run are still claimed or running: its end did not return them
    /// yet. Recovery returns them (`State::return_work`).
    WorkLeft(u32),
    /// Members at once asked for are 0 or above the setting.
    PastCeiling {
        ceiling: u32,
    },
    /// The team ended, but returning its work failed. The end stands; recovery returns
    /// the work.
    Plan(PlanError),
    Store(anyhow::Error),
}

/// How the store and the protocol's `TeamEnd` name a reason.
pub(super) fn end_name(end: TeamEnd) -> &'static str {
    match end {
        TeamEnd::Done => "done",
        TeamEnd::Steps => "steps",
        TeamEnd::Tokens => "tokens",
        TeamEnd::Time => "time",
        TeamEnd::Stalled => "stalled",
        TeamEnd::Stopped => "stopped",
        TeamEnd::Quiet => "quiet",
    }
}

pub(super) fn parse_end(text: &str) -> Option<TeamEnd> {
    [
        TeamEnd::Done,
        TeamEnd::Steps,
        TeamEnd::Tokens,
        TeamEnd::Time,
        TeamEnd::Stalled,
        TeamEnd::Stopped,
        TeamEnd::Quiet,
    ]
    .into_iter()
    .find(|end| end_name(*end) == text)
}

impl fmt::Display for TeamError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoSuchThread(_) => write!(f, "no such thread"),
            Self::NoTeam => write!(f, "no team has run in this chat"),
            Self::Running => write!(f, "the team is running already"),
            Self::Ended(why) => write!(f, "the team is not running; it ended: {}", end_name(*why)),
            Self::WorkLeft(count) => write!(
                f,
                "{count} tasks of the last run are still at work; they go back to open first"
            ),
            Self::PastCeiling { ceiling } => {
                write!(f, "members at once must be 1 to {ceiling}")
            }
            Self::Plan(error) => write!(f, "the team ended; its work was not returned: {error}"),
            Self::Store(error) => write!(f, "the team's store failed: {error:#}"),
        }
    }
}

impl std::error::Error for TeamError {}

impl From<PlanError> for TeamError {
    fn from(error: PlanError) -> Self {
        match error {
            PlanError::NoSuchThread(thread) => Self::NoSuchThread(thread),
            PlanError::Store(error) => Self::Store(error),
            other => Self::Plan(other),
        }
    }
}

impl From<sqlx::Error> for TeamError {
    fn from(error: sqlx::Error) -> Self {
        Self::Store(error.into())
    }
}

impl From<anyhow::Error> for TeamError {
    fn from(error: anyhow::Error) -> Self {
        Self::Store(error)
    }
}
