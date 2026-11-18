//! A member: a thread the core makes under the lead for one task. What it may do is
//! fixed here, when the task is claimed, and nothing a model writes changes it: not the
//! role (a role gives a prompt, never powers), not the lead's plan, not the member.
//!
//! | Thing | Value | Where |
//! |---|---|---|
//! | tools | its role's, never `DENIED` | `limit_tools`, when its runner is built |
//! | folder | the lead's project folder | `give`, as the folder its card is delivered in |
//! | approval mode | the lead thread's, copied | `give`, before the claim |
//! | model | `team.models[role]`, else `team.model`, else the chat's | `model_name`, into the claim |
//! | depth | 1, and no `ask_agent` | the thread's parent is the lead; `DENIED` |
//!
//! The order of `give`, and what a stop between two steps leaves:
//! 1. The thread is made (or a spare is taken) and its approval mode is set. A stop
//!    here leaves a spare: it has no task and no message, so nothing runs, and the next
//!    claim of its role takes it (`State::spare_member`).
//! 2. The claim, one statement. A stop here leaves a claimed task without a card: the
//!    next pass delivers it, and without one the lease runs out and the task reopens.
//! 3. The card is delivered under `card:<task>:<attempt>`, which stores it once however
//!    often this is run, and the member's runner is started.

use std::path::Path;

use anyhow::{Result, ensure};
use tracing::{debug, info};

use super::card;
use crate::{
    catalog::{agents::GENERAL_AGENT, skills},
    config::Config,
    runtime::{
        Launcher,
        model_choice::Choice,
        thread::start::{read_prompt, start_member_thread},
    },
    state::{Claim, MAX_REWORKS, MEMBER_SOURCE, PlanError, PlanTask, TeamRun, Thread},
    tools::{ToolRegistry, short_task_id},
};

/// What no member has, whatever its role lists: a member starts no agent, changes no
/// plan, writes no memory, and reports through the core, not by its own message.
const DENIED: [&str; 6] = [
    "ask_agent",
    "tasks",
    "remember",
    "remember_project",
    "message_parent",
    "agent_state",
];

/// Whether the team made this thread for a task.
pub(crate) fn is_member(thread: &Thread) -> bool {
    thread.source == MEMBER_SOURCE
}

/// Cuts a member's tools to what a member may have. K1 turns this into the member set
/// of section 9, an allowlist; until then it is the role's tools without `DENIED`.
pub(crate) fn limit_tools(tools: &mut ToolRegistry) {
    let kept: Vec<String> = tools
        .kinds()
        .into_iter()
        .map(|(name, _)| name)
        .filter(|name| !DENIED.contains(&name.as_str()))
        .collect();
    tools.keep(&kept);
}

/// The model a member of `role` answers on, as `provider/model`: the user's setting for
/// the role, else for the team, else the chat's. Recorded with the claim; M0 makes the
/// member's runner ask it.
pub(super) fn model_name(config: &Config, role: &str, chat: Option<&Choice>) -> Option<String> {
    let team = config.team();
    team.models
        .get(role)
        .or(team.model.as_ref())
        .cloned()
        .or_else(|| chat.map(Choice::name))
}

/// How giving a task out went.
pub(super) enum Given {
    Claimed(Box<PlanTask>),
    /// The claim did not take the task: another holds its files, or it changed
    /// meanwhile. Not an error; the thread made for it is a spare now.
    Lost,
    /// As many agents are at work as `max_agents_at_work` allows.
    NoSlot,
    /// The task's role cannot be worked as.
    NoAgent(String),
}

/// Gives `task` to a member, in the order the module's doc states.
pub(super) async fn give(
    launcher: &Launcher,
    lead: &str,
    run: &TeamRun,
    task: &PlanTask,
    folder: &Path,
    now: i64,
) -> Result<Given> {
    let state = &launcher.state;
    let config = launcher.config.current();
    // Kept until the card is delivered, which gives the member its slot.
    let admitted = match launcher.admit_agent().await {
        Ok(admitted) => admitted,
        Err(error) => {
            debug!("team of thread {lead}: no member starts now: {error:#}");
            return Ok(Given::NoSlot);
        }
    };
    let role = task.role.as_deref().unwrap_or(GENERAL_AGENT);
    let member = match state.spare_member(lead, role).await? {
        Some(spare) => spare,
        None => match make(launcher, &config, lead, role).await {
            Ok(made) => made,
            Err(error) => return Ok(Given::NoAgent(format!("{error:#}"))),
        },
    };
    // Copied, never raised: a member asks for no more than its lead may do unasked.
    let policy = match state.thread_approvals(lead).await? {
        Some(own) => own,
        None => config.approvals(),
    };
    state.set_thread_approvals(&member, Some(policy)).await?;
    let model = model_name(&config, role, launcher.model.current().as_ref());
    let claim = Claim {
        owner: &member,
        at_once: run.budget.at_once(),
        model: model.as_deref(),
        now,
    };
    let claimed = match state.claim_task(lead, &task.task_id, claim).await {
        Ok(claimed) => claimed,
        Err(PlanError::NotClaimable { why, .. }) => {
            debug!("task {} was not claimed: {why:?}", task.task_id);
            return Ok(Given::Lost);
        }
        Err(PlanError::Illegal { .. }) => return Ok(Given::Lost),
        Err(error) => return Err(error.into()),
    };
    info!(
        "task {} given to member {member} as {role}, attempt {}",
        claimed.task_id, claimed.attempts
    );
    deliver_card(launcher, lead, &member, &claimed, folder).await?;
    drop(admitted);
    Ok(Given::Claimed(Box::new(claimed)))
}

/// A new thread under the lead that works as `role`, with the role's usual prompt.
async fn make(launcher: &Launcher, config: &Config, lead: &str, role: &str) -> Result<String> {
    let paths = &launcher.paths;
    let agents = &launcher.agents;
    let prompt = if role == GENERAL_AGENT && agents.builtin_general(config) {
        read_prompt(&paths.main_prompt())?
    } else {
        ensure!(
            agents.get(role).is_some() && config.agent_enabled(role),
            "no agent {role} to give it to"
        );
        let skills = skills::list(&paths.skills_dir(), config);
        Some(agents.prompt(role, &skills).await?.1)
    };
    let thread = start_member_thread(&launcher.state, paths, lead, role, prompt).await?;
    Ok(thread.thread_id)
}

/// Delivers the card of a claimed task to its member and starts it; run again, it
/// stores nothing and only starts a member that still waits.
pub(super) async fn deliver_card(
    launcher: &Launcher,
    lead: &str,
    member: &str,
    task: &PlanTask,
    folder: &Path,
) -> Result<()> {
    let text = card::card(task);
    launcher
        .deliver_once(member, lead, &card::uid(task), &text, folder)
        .await?;
    Ok(())
}

/// Wakes the member of a task the lead sent back from review, with what to redo. The
/// note is the lead's own text. Once for each time it came back.
pub(super) async fn deliver_rework(
    launcher: &Launcher,
    lead: &str,
    member: &str,
    task: &PlanTask,
    folder: &Path,
) -> Result<()> {
    let uid = format!("rework:{}:{}", task.task_id, task.reworks);
    let text = format!(
        "Task #{} came back from review ({} of {MAX_REWORKS}). Redo:\n{}",
        short_task_id(&task.task_id),
        task.reworks,
        task.note
    );
    launcher
        .deliver_once(member, lead, &uid, &text, folder)
        .await?;
    Ok(())
}
