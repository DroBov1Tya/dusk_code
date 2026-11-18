//! The team: the core's run of a chat's plan.
//! No model call is made here. A change to the plan or the team, a member's turn and a
//! 30 s tick each lead to one `dispatch::pass`, which reads the tables and does what they
//! ask for: leases that ran out are released, budgets are checked, ready tasks are given
//! to members, and the team ends when it is done, spent or stalled.
//!
//! `member` makes a member's thread and fixes what it may do, `allow` is the user's one
//! yes for the plan's gates, `card` writes its first message, `clean` takes secrets out
//! of every text that is forwarded, `lease` keeps its claim while its runner lives,
//! `hand_in` moves its task when its turn ends, `outside` finds what it changed beyond
//! its files, `gate` is where a command gate starts, `lead_lines` is what the lead is
//! told, `end` ends a team, `settle` recovers one after a restart, `events` tells the
//! clients.
//!
//! Everything it needs to go on is in the database. `Teams` holds a lock and a few caches.

mod allow;
mod card;
mod clean;
mod dispatch;
mod end;
mod events;
mod gate;
mod hand_in;
mod lead_lines;
mod lease;
mod member;
mod outside;
mod settle;
#[cfg(test)]
mod tests;

use std::{
    collections::{HashMap, HashSet},
    time::{Duration, Instant},
};

use anyhow::{Context, Result, ensure};
use dusk_protocol::thread::{Team, TeamBudget, TeamEnd, TeamStatus};
use tokio::task::JoinHandle;
use tracing::warn;

use crate::{
    BoxFuture,
    catalog::modes::TEAM,
    runtime::Launcher,
    state::{BudgetBy, PlanTask, TeamCeilings, TeamError, timestamp_ms},
    tools::{Progress, Started, TeamRunner},
};

pub(crate) use hand_in::member_event;
pub(crate) use lease::{Beat, may_step};
pub(crate) use member::{is_member, limit_tools, model as member_model};

/// How often the dispatcher looks at every running team by itself.
const TICK: Duration = Duration::from_secs(30);

/// What the team's runtime keeps in memory, one for every chat. Losing it loses nothing:
/// after a restart a pass goes on from the tables alone.
#[derive(Default)]
pub struct Teams {
    /// One pass at a time: making a member's thread and claiming for it are two writes,
    /// and two passes at once would make two threads for one task.
    pass: tokio::sync::Mutex<()>,
    /// The line each lead was last woken with.
    told: parking_lot::Mutex<HashMap<String, lead_lines::Told>>,
    /// When each lead's clients last heard of its team: at most one event a second.
    heard: parking_lot::Mutex<HashMap<String, Instant>>,
    /// The gate commands the user allowed for each chat's team, exactly as written.
    allowed: parking_lot::Mutex<HashMap<String, HashSet<String>>>,
    /// The tasks whose gate runs now, so a pass starts none twice. After a restart it
    /// is empty and a gate that was cut off runs again: a gate is a check.
    gates: parking_lot::Mutex<HashMap<String, gate::Running>>,
    /// The gates each running team has run, against `limits.team_max_gate_runs`. After
    /// a restart it counts again from nothing; the team's time still bounds it.
    gate_runs: parking_lot::Mutex<HashMap<String, u32>>,
    /// The members a runner of this process has reported for. Recovery settles only
    /// the others: what a stopped process left.
    served: parking_lot::Mutex<HashSet<String>>,
}

/// The team behind a chat's `tasks`: what its `run`, `stop` and `show` reach.
pub struct Runner {
    launcher: Launcher,
}

impl Runner {
    pub fn new(launcher: Launcher) -> Self {
        Self { launcher }
    }

    /// Whether the chat `lead` may run a team now: the switch is on, the chat is in TEAM
    /// mode, and it is a chat.
    async fn may_lead(&self, lead: &str) -> Result<()> {
        let launcher = &self.launcher;
        ensure!(
            launcher.config.current().limits().plan_graph,
            "the team is switched off (limits.plan_graph); the plan is kept"
        );
        ensure!(
            in_team_mode(launcher, lead).await?,
            "a team runs only in TEAM mode; the plan is kept"
        );
        let chat = launcher
            .state
            .load_thread(lead)
            .await?
            .with_context(|| format!("thread {lead} does not exist"))?;
        ensure!(
            chat.parent_thread_id.is_none(),
            "only a chat leads a team; an agent's thread cannot"
        );
        Ok(())
    }

    /// Starts the chat's team, or goes on with the one running: a part of its budget the
    /// lead now names lower is lowered, none is raised.
    async fn start(&self, lead: &str, budget: TeamBudget, now: i64) -> Result<()> {
        let launcher = &self.launcher;
        let config = launcher.config.current();
        let state = &launcher.state;
        let ceilings = || TeamCeilings::from_limits(&config.limits());
        let _one = launcher.team.pass.lock().await;
        match state.team_run(lead).await? {
            Some(run) if run.status == TeamStatus::Running => {
                let lower = TeamBudget::new(
                    run.budget.at_once().min(budget.at_once()),
                    run.budget.steps().min(budget.steps()),
                    run.budget.tokens().min(budget.tokens()),
                    run.budget.minutes().min(budget.minutes()),
                )?;
                if lower != run.budget {
                    state
                        .set_team_budget(lead, BudgetBy::Lead, lower, ceilings()?)
                        .await?;
                }
            }
            _ => match state.start_team(lead, budget, ceilings()?, now).await {
                Ok(_) => {}
                // The last end was cut off before it gave its work back.
                Err(TeamError::WorkLeft(_)) => {
                    for task in state.return_work(lead).await? {
                        events::task(launcher, lead, &task).await;
                    }
                    state.start_team(lead, budget, ceilings()?, now).await?;
                }
                Err(error) => return Err(error.into()),
            },
        }
        Ok(())
    }
}

impl TeamRunner for Runner {
    fn run<'a>(
        &'a self,
        lead: &'a str,
        budget: TeamBudget,
        progress: Option<Progress>,
    ) -> BoxFuture<'a, Started> {
        Box::pin(async move {
            self.may_lead(lead).await?;
            // Before the start, so no member works before the user answered.
            let gates = allow::ask(&self.launcher, lead, progress).await?;
            let now = timestamp_ms()?;
            self.start(lead, budget, now).await?;
            events::team(&self.launcher, lead, now, true).await;
            let given = dispatch::pass(&self.launcher, lead, now).await?;
            Ok(Started { given, gates })
        })
    }

    fn stop<'a>(&'a self, lead: &'a str) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            match self.launcher.team_stop(lead).await {
                Ok(_) | Err(TeamError::NoTeam | TeamError::Ended(_)) => Ok(()),
                Err(error) => Err(error.into()),
            }
        })
    }

    fn team<'a>(&'a self, lead: &'a str) -> BoxFuture<'a, Option<Team>> {
        Box::pin(async move { Ok(self.launcher.state.team(lead, timestamp_ms()?).await?) })
    }

    fn changed<'a>(&'a self, lead: &'a str) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            self.launcher.team_changed(lead).await;
            Ok(())
        })
    }
}

impl Launcher {
    /// The plan or the team of the chat `lead` changed: its clients hear of the team, and
    /// a running team gives out what is ready now. Whoever changes a task or a team from
    /// outside this module calls this afterwards: the lead's tool, a client's request.
    pub async fn team_changed(&self, lead: &str) {
        let Ok(now) = timestamp_ms() else { return };
        if let Err(error) = self.team_pass_at(lead, now).await {
            warn!("team of thread {lead}: {error:#}");
        }
        events::team(self, lead, now, false).await;
    }

    /// The chat `lead` was opened: what a stopped core left of its team is settled, then
    /// a pass goes on with it. Called before the chat's agents are woken, so a member
    /// whose turn can go on finds its lease started again.
    pub async fn team_settle(&self, lead: &str) {
        if !self.config.current().limits().plan_graph {
            return;
        }
        if let Err(error) = settle::settle(self, lead).await {
            warn!("team of thread {lead}: cannot settle it: {error:#}");
        }
        self.team_changed(lead).await;
    }

    /// Ends the chat's running team as stopped: its members at work are cancelled, their
    /// tasks go back to open (answered here), and the lead reads of it with its next
    /// request. A team that is not running is the store's refusal.
    pub async fn team_stop(&self, lead: &str) -> Result<Vec<PlanTask>, TeamError> {
        let now = timestamp_ms()?;
        let _one = self.team.pass.lock().await;
        end::end(self, lead, TeamEnd::Stopped, now).await
    }

    /// One pass over the chat's team as of `now`; answers how many tasks it gave out.
    pub(crate) async fn team_pass_at(&self, lead: &str, now: i64) -> Result<usize> {
        dispatch::pass(self, lead, now).await
    }

    /// The dispatcher's own look at every running team whose chat is open. A team under
    /// a chat that is not open does nothing, as its agents do nothing.
    async fn team_tick(&self) {
        if !self.config.current().limits().plan_graph {
            return;
        }
        let leads = match self.state.running_teams().await {
            Ok(leads) => leads,
            Err(error) => return warn!("cannot list the running teams: {error:#}"),
        };
        for lead in leads {
            if self.mailboxes.get(&lead).await.is_some() {
                self.team_changed(&lead).await;
            }
        }
    }
}

/// Whether the chat `lead` is in TEAM mode: its own mode, the one its user last wrote
/// to it in, else the setting's. A switch made for another chat does not pause its team.
pub(super) async fn in_team_mode(launcher: &Launcher, lead: &str) -> Result<bool> {
    let mode = match launcher.state.chat_mode(lead).await? {
        Some(mode) => mode,
        None => launcher.config.current().mode().to_owned(),
    };
    Ok(mode == TEAM)
}

/// The dispatcher's tick, started with the core and aborted when it shuts down.
pub fn ticker(launchers: Vec<Launcher>) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(TICK);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tick.tick().await;
            for launcher in &launchers {
                launcher.team_tick().await;
            }
        }
    })
}
