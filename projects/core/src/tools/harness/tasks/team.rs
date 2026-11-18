//! The seam between the lead's `tasks` and the team that runs a plan. The team runtime
//! implements `TeamRunner` and the launcher hands it to a chat's `tasks` with
//! `Tasks::lead`; `run`, `stop` and the budget line of `show` reach the team only here.

use dusk_protocol::thread::{Team, TeamBudget};

use crate::BoxFuture;

/// What starts, stops and reports the team of a plan. `lead` is the chat's root thread.
pub trait TeamRunner: Send + Sync {
    /// Starts the team of the plan with `budget`, already held to the ceilings, or goes
    /// on with the one running; a second call with a team running changes nothing, since
    /// a restart runs it again. Answers how many tasks it gave out now.
    fn run<'a>(&'a self, lead: &'a str, budget: TeamBudget) -> BoxFuture<'a, usize>;

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
    fn run<'a>(&'a self, _lead: &'a str, _budget: TeamBudget) -> BoxFuture<'a, usize> {
        Box::pin(async { anyhow::bail!(NOT_BUILT) })
    }

    fn stop<'a>(&'a self, _lead: &'a str) -> BoxFuture<'a, ()> {
        Box::pin(async { anyhow::bail!(NOT_BUILT) })
    }

    fn team<'a>(&'a self, _lead: &'a str) -> BoxFuture<'a, Option<Team>> {
        Box::pin(async { Ok(None) })
    }
}
