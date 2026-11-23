//! Recovery: what a stopped core left of a chat's team, settled once when the chat is
//! opened again and before its agents are woken. Nothing in memory is needed for it; the
//! tables say where every task stands, and each case below ends in a state the
//! dispatcher's pass already knows how to go on from.
//!
//! | Found | Settled as |
//! |---|---|
//! | a team that ended with tasks still at work | the tasks go back to open |
//! | a member's turn still marked running | its lease starts again; the member is woken with the chat's other agents and its turn goes on, or is stopped, and either is then its own event |
//! | a member whose card or newest rework note no ended turn has read | nothing: the pass delivers it and starts the member |
//! | a turn that completed, its task not handed in | handed in now, from that turn's answer |
//! | a turn that failed or was stopped, its task not released | released: back to open, or failed at the third attempt |
//! | a task in review for a command gate | nothing: the pass runs the gate again |
//!
//! A member that a runner of this process serves is left alone: its own events move
//! its task, and settling it here too could move a task twice. Every move is the
//! store's, checked against the owner and the status, so a task is handed in once,
//! released once, and never both.

use anyhow::Result;
use dusk_protocol::thread::{TaskStatus, TeamStatus};
use tracing::info;

use super::{clean, dispatch, events, hand_in, member};
use crate::{
    runtime::Launcher,
    state::{Delivery, TurnStatus, timestamp_ms},
};

const CUT_OFF: &str = "a restart cut its turn off";

/// Settles the team of the chat `lead`. The caller follows it with a pass.
pub(super) async fn settle(launcher: &Launcher, lead: &str) -> Result<()> {
    let state = &launcher.state;
    let _one = launcher.team.pass.lock().await;
    let Some(run) = state.team_run(lead).await? else {
        return Ok(());
    };
    if run.status != TeamStatus::Running {
        // The end was stored and the stop came before its work was given back.
        if state.team_counts(lead).await?.at_work > 0 {
            for task in state.return_work(lead).await? {
                events::task(launcher, lead, &task).await;
            }
        }
        return Ok(());
    }
    for held in state.team_members(lead).await? {
        if !matches!(held.status, TaskStatus::Claimed | TaskStatus::Running) {
            continue;
        }
        let member = &held.thread_id;
        if launcher.team.served.lock().contains(member) {
            continue;
        }
        let Some(turn) = state.last_turn(member).await? else {
            continue;
        };
        if turn.status == TurnStatus::Running {
            state
                .renew_lease(member, &held.task_id, timestamp_ms()?)
                .await?;
            continue;
        }
        // Only a turn that read what the member was last sent, its card or the note
        // of its newest rework, says how this round of the task ended. Anything older
        // is the round before, and the pass delivers what is still unread.
        let task = state.plan_task(lead, &held.task_id).await?;
        let read = state.delivery(&member::round_uid(&task)).await?;
        if read != (Delivery::Read { answered: true }) {
            continue;
        }
        match turn.status {
            TurnStatus::Completed => {
                let Some(thread) = state.load_thread(member).await? else {
                    continue;
                };
                info!("task {} is handed in after a restart", held.task_id);
                hand_in::completed(launcher, lead, &thread, &turn).await?;
            }
            _ => {
                let note = clean::note(launcher, lead, CUT_OFF).await;
                let task = state.release_task(lead, &held.task_id, &note).await?;
                info!("task {} is released after a restart", held.task_id);
                dispatch::released(launcher, lead, &task).await;
            }
        }
    }
    Ok(())
}
