//! The user's one yes for a plan's gates. A gate runs where nobody can be asked, so in a
//! mode that asks for each command the lead's `run` asks the user once, with every gate
//! command of the plan listed. The yes covers exactly those lines, for this team: a gate
//! written or changed later is not covered until a `run` asks again. A no leaves the
//! gates unrun, and their tasks wait for the lead.
//!
//! Like a yes for the session, it lives in memory only: after a restart a gate that
//! needs it waits for the lead until `run` asks again.

use std::collections::BTreeSet;

use anyhow::Result;
use tracing::warn;

use crate::{
    config::ApprovalPolicy,
    runtime::Launcher,
    state::{GoalCheck, PlanStatus},
    tools::{GatesYes, Progress},
};

/// Asks the user to allow the plan's gate commands that this team may not run yet, when
/// the chat's approval mode would ask for each command.
pub(super) async fn ask(
    launcher: &Launcher,
    lead: &str,
    progress: Option<Progress>,
) -> Result<GatesYes> {
    if policy(launcher, lead).await != ApprovalPolicy::Ask {
        return Ok(GatesYes::NotAsked);
    }
    let asked = unallowed(launcher, lead).await?;
    if asked.is_empty() {
        return Ok(GatesYes::NotAsked);
    }
    match launcher
        .approvals
        .ask_commands(lead, &asked, progress)
        .await
    {
        Ok(()) => {
            let mut allowed = launcher.team.allowed.lock();
            allowed.entry(lead.to_owned()).or_default().extend(asked);
            Ok(GatesYes::Allowed)
        }
        Err(error) => {
            warn!("the gates of thread {lead} were not allowed: {error:#}");
            Ok(GatesYes::Refused)
        }
    }
}

/// Whether the user allowed `command`, exactly as written, as a gate of this team.
pub(super) fn covers(launcher: &Launcher, lead: &str, command: &str) -> bool {
    let allowed = launcher.team.allowed.lock();
    allowed
        .get(lead)
        .is_some_and(|commands| commands.contains(command))
}

/// The team ended: its yes ends with it.
pub(super) fn forget(launcher: &Launcher, lead: &str) {
    launcher.team.allowed.lock().remove(lead);
}

/// The distinct gate commands of the tasks still to do that the yes does not cover, in
/// a stable order.
async fn unallowed(launcher: &Launcher, lead: &str) -> Result<Vec<String>> {
    let plan = launcher.state.plan(lead).await?;
    let commands: BTreeSet<String> = plan
        .into_iter()
        .filter(|task| !matches!(task.status, PlanStatus::Done | PlanStatus::Failed))
        .filter_map(|task| match task.gate {
            Some(GoalCheck::Command { command }) => Some(command),
            _ => None,
        })
        .filter(|command| !covers(launcher, lead, command))
        .collect();
    Ok(commands.into_iter().collect())
}

/// The chat's approval mode as its commands are judged: its own, else the settings'.
async fn policy(launcher: &Launcher, lead: &str) -> ApprovalPolicy {
    let config = launcher.config.current();
    match launcher.state.thread_approvals(lead).await {
        Ok(Some(policy)) => policy,
        Ok(None) => config.approvals(),
        Err(error) => {
            warn!("cannot read the approval mode of thread {lead}: {error:#}");
            config.approvals()
        }
    }
}
