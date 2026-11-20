//! The seam between the lead's `tasks` and the team that runs a plan. The team runtime
//! implements `TeamRunner` and the launcher hands it to a chat's `tasks` with
//! `Tasks::lead`; `run`, `stop` and the budget line of `show` reach the team only here.

use dusk_protocol::thread::{Team, TeamBudget};

use crate::{BoxFuture, tools::Progress};

/// What `run` did: the tasks it gave out now, and how the user answered for the gates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Started {
    pub given: usize,
    pub gates: GatesYes,
}

/// How the user answered when `run` asked once for the plan's gate commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GatesYes {
    /// Nothing was asked: no gate needed it, or the mode does not ask for commands.
    NotAsked,
    Allowed,
    Refused,
}

/// What starts, stops and reports the team of a plan. `lead` is the chat's root thread.
pub trait TeamRunner: Send + Sync {
    /// Starts the team of the plan with `budget`, already held to the ceilings, or goes
    /// on with the one running; a second call with a team running changes nothing, since
    /// a restart runs it again. In a mode that asks for each command, the user is asked
    /// once, through `progress`, to allow the plan's gate commands.
    fn run<'a>(
        &'a self,
        lead: &'a str,
        budget: TeamBudget,
        progress: Option<Progress>,
    ) -> BoxFuture<'a, Started>;

    /// Ends the running team as stopped: members at work are cancelled and their tasks
    /// go back to open. A team that is not running is no error.
    fn stop<'a>(&'a self, lead: &'a str) -> BoxFuture<'a, ()>;

    /// The plan's team, running or ended; `None` before its first run.
    fn team<'a>(&'a self, lead: &'a str) -> BoxFuture<'a, Option<Team>>;

    /// The lead changed its plan (a review, a reopen, new tasks): a running team gives
    /// out what became ready at once instead of at its next look.
    fn changed<'a>(&'a self, _lead: &'a str) -> BoxFuture<'a, ()> {
        Box::pin(async { Ok(()) })
    }
}

/// Until the team runtime lands: `run` and `stop` say so, `show` has no budget line.
pub struct NoTeam;

const NOT_BUILT: &str = "the team runner is not built yet; the plan is kept, and \
                         ask_agent can do its tasks one by one meanwhile";

impl TeamRunner for NoTeam {
    fn run<'a>(
        &'a self,
        _lead: &'a str,
        _budget: TeamBudget,
        _progress: Option<Progress>,
    ) -> BoxFuture<'a, Started> {
        Box::pin(async { anyhow::bail!(NOT_BUILT) })
    }

    fn stop<'a>(&'a self, _lead: &'a str) -> BoxFuture<'a, ()> {
        Box::pin(async { anyhow::bail!(NOT_BUILT) })
    }

    fn team<'a>(&'a self, _lead: &'a str) -> BoxFuture<'a, Option<Team>> {
        Box::pin(async { Ok(None) })
    }
}
