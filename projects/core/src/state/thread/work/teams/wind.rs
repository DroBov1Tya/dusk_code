//! A team that winds down: it gives out nothing new while the members at work hand over,
//! then ends. Only the start of it is stored here; the end is `end_team`, as for any team.

use dusk_protocol::thread::{TeamEnd, TeamStatus};

use super::{State, TeamError, read_run, root_of};

/// Why a team winds down.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindWhy {
    /// The lead or the user on the Team page stopped it.
    Stopped,
    /// The lead was written to as often as a chat may be without the user.
    Quiet,
    /// The provider keeps failing the members' turns.
    Provider,
}

impl WindWhy {
    /// How the team ends once it has wound down. A provider that failed ends it as
    /// stopped, a reason every client knows; the end line says why.
    pub fn end(self) -> TeamEnd {
        match self {
            Self::Stopped | Self::Provider => TeamEnd::Stopped,
            Self::Quiet => TeamEnd::Quiet,
        }
    }

    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Stopped => "stopped",
            Self::Quiet => "quiet",
            Self::Provider => "provider",
        }
    }

    pub(super) fn parse(text: &str) -> Option<Self> {
        [Self::Stopped, Self::Quiet, Self::Provider]
            .into_iter()
            .find(|why| why.name() == text)
    }
}

/// Since when and why a team winds down.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Winding {
    /// Unix ms.
    pub since: i64,
    pub why: WindWhy,
}

impl State {
    /// Marks the chat's running team as winding down from `now`. Answers false when it
    /// already was, which keeps the first reason and time.
    pub async fn wind_team(&self, thread: &str, why: WindWhy, now: i64) -> Result<bool, TeamError> {
        let mut transaction = self.pool.begin().await?;
        let root = root_of(&mut transaction, thread).await?;
        let run = read_run(&mut transaction, &root)
            .await?
            .ok_or(TeamError::NoTeam)?;
        if let TeamStatus::Ended(before) = run.status {
            return Err(TeamError::Ended(before));
        }
        let wound = sqlx::query(
            "UPDATE teams SET winding_since = ?, winding_why = ?
             WHERE thread_id = ? AND status = 'running' AND winding_since IS NULL",
        )
        .bind(now)
        .bind(why.name())
        .bind(&root)
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        transaction.commit().await?;
        Ok(wound == 1)
    }
}
