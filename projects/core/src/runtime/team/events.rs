//! What a chat's clients hear of its plan and its team: `TaskChanged` for every task this
//! module moved, `Team` when the team's status, members or budget changed. Both go out
//! through the lead's own thread, so only a chat that is open hears them; a client that
//! opens it later reads the plan and the team with its requests.

use std::time::{Duration, Instant};

use tracing::warn;

use crate::{
    runtime::{Launcher, ThreadEvent},
    state::PlanTask,
};

/// The least time between two `Team` events of one chat.
const TEAM_EVERY: Duration = Duration::from_secs(1);

/// Tells the lead's clients how `task` stands now.
pub(super) async fn task(launcher: &Launcher, lead: &str, task: &PlanTask) {
    let Some(mailbox) = launcher.mailboxes.get(lead).await else {
        return;
    };
    match launcher.state.present_task(task.clone()).await {
        Ok(shown) => mailbox.emit(ThreadEvent::TaskChanged(shown)),
        Err(error) => warn!("cannot show task {}: {error}", task.task_id),
    }
}

/// Tells the lead's clients how its team stands. Unless `always`, an event within a
/// second of the last is left out: the next change or the tick sends the newer state.
pub(super) async fn team(launcher: &Launcher, lead: &str, now: i64, always: bool) {
    let Some(mailbox) = launcher.mailboxes.get(lead).await else {
        return;
    };
    {
        let mut heard = launcher.team.heard.lock();
        let recent = heard
            .get(lead)
            .is_some_and(|last| last.elapsed() < TEAM_EVERY);
        if recent && !always {
            return;
        }
        heard.insert(lead.to_owned(), Instant::now());
    }
    match launcher.state.team(lead, now).await {
        Ok(Some(team)) => mailbox.emit(ThreadEvent::Team(team)),
        Ok(None) => {}
        Err(error) => warn!("cannot read the team of thread {lead}: {error}"),
    }
}
