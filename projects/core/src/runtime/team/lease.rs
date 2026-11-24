//! A member's claim while it works. Its runner moves the lease on every 30 s for as long
//! as it lives; the statement names the member's thread, so no other thread and no tool
//! can hold a task it was not given. Before each step the member's team is asked
//! whether it still runs, which is what stops a member of a team that ended.

use std::time::Duration;

use anyhow::{Result, bail, ensure};
use dusk_protocol::thread::{TaskStatus, TeamEnd, TeamMember};
use tracing::debug;

use super::member::{is_member, lead_policy};
use crate::{
    config::Config,
    runtime::{Launcher, ThreadEvent, ThreadOutput},
    state::{State, TeamAsk, TeamCheck, Thread, timestamp_ms},
};

/// How often a living runner moves its lease on; the lease is `LEASE_MS`, four of these.
const BEAT: Duration = Duration::from_secs(30);

/// The heartbeat of one member's runner.
pub(crate) struct Beat(tokio::time::Interval);

impl Beat {
    pub fn new() -> Self {
        let mut every = tokio::time::interval(BEAT);
        every.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        Self(every)
    }

    /// The runner's next event; while it waits, the lease is moved on at every beat.
    /// `None` once the runner has ended, and with it the heartbeat.
    pub async fn next_event(
        &mut self,
        launcher: &Launcher,
        member: &Thread,
        output: &mut ThreadOutput,
    ) -> Option<ThreadEvent> {
        loop {
            tokio::select! {
                event = output.next_event() => return event,
                _ = self.0.tick() => renew(launcher, member).await,
            }
        }
    }
}

async fn renew(launcher: &Launcher, member: &Thread) {
    let state = &launcher.state;
    let Some(lead) = member.parent_thread_id.as_deref() else {
        return;
    };
    let renewed = async {
        let Some(held) = at_work(state, lead, &member.id).await? else {
            return Ok(());
        };
        state
            .renew_lease(&member.id, &held.task_id, timestamp_ms()?)
            .await?;
        anyhow::Ok(())
    };
    if let Err(error) = renewed.await {
        debug!(
            "member {}: its lease was not moved on: {error:#}",
            member.id
        );
    }
}

/// The task `member` holds of the plan `lead` leads, wherever it stands.
pub(super) async fn held(state: &State, lead: &str, member: &str) -> Result<Option<TeamMember>> {
    let members = state.team_members(lead).await?;
    Ok(members.into_iter().find(|held| held.thread_id == member))
}

/// The task `member` holds claimed or running.
pub(super) async fn at_work(state: &State, lead: &str, member: &str) -> Result<Option<TeamMember>> {
    let held = held(state, lead, member).await?;
    Ok(held.filter(|held| matches!(held.status, TaskStatus::Claimed | TaskStatus::Running)))
}

/// Whether the thread may take another model step. Only a team's member is ever
/// refused: when it holds no task at work (its task was taken back, or it was written
/// to after it finished), when its team ended, and when its team winds down and the
/// grace for its hand-over ran out. The refusal fails its turn.
pub(crate) async fn may_step(
    state: &State,
    config: &Config,
    thread_id: &str,
    depth: usize,
) -> Result<()> {
    if depth != 1 || !config.limits().plan_graph {
        return Ok(());
    }
    let Some(thread) = state.load_thread(thread_id).await? else {
        return Ok(());
    };
    let Some(lead) = thread
        .parent_thread_id
        .as_deref()
        .filter(|_| is_member(&thread))
    else {
        return Ok(());
    };
    ensure!(
        at_work(state, lead, thread_id).await?.is_some(),
        "this member holds no task at work: its task was taken back or is finished"
    );
    // The lead's mode as it is now: one the user tightened since the claim holds from
    // the member's next step on.
    let policy = lead_policy(state, config, lead).await?;
    if state.thread_approvals(thread_id).await? != Some(policy) {
        state.set_thread_approvals(thread_id, Some(policy)).await?;
    }
    match state.team_check(lead, TeamAsk::Step).await? {
        TeamCheck::Go | TeamCheck::Full => Ok(()),
        TeamCheck::Winding(since) => {
            let grace = config.limits().team_wrap_seconds.saturating_mul(1000);
            let until = since.saturating_add(i64::try_from(grace).unwrap_or(i64::MAX));
            ensure!(timestamp_ms()? < until, "the team was stopped");
            Ok(())
        }
        TeamCheck::Ended(TeamEnd::Stopped) => bail!("the team was stopped"),
        TeamCheck::Ended(_) => bail!("the team ended"),
    }
}
