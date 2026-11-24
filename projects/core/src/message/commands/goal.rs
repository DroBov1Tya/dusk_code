//! `/goal <condition>` sets the chat's goal and starts a turn on it at once; `/goal`
//! alone shows it, and `/goal clear` (or stop, off, reset, none, cancel) removes it.

use crate::runtime::{
    ThreadEvent,
    agent::Agent,
    goal::command::{self as goal, GoalAction},
};

pub async fn run(agent: &Agent, argument: Option<&str>) {
    let action = match GoalAction::from_command(argument) {
        Ok(action) => action,
        Err(error) => {
            agent.emit(ThreadEvent::Rejected {
                reason: format!("{error:#}"),
            });
            return;
        }
    };
    let applied = async {
        let applied = goal::apply(&agent.state, &agent.thread_id, action).await?;
        if let Some(started) = &applied.started {
            agent.state.add_pending(&agent.thread_id, started).await?;
        }
        anyhow::Ok(applied)
    };
    match applied.await {
        Ok(applied) => agent.emit(ThreadEvent::Goal {
            goal: applied.goal,
            note: applied.note,
            started: applied.started,
        }),
        Err(error) => agent.emit(ThreadEvent::Rejected {
            reason: format!("{error:#}"),
        }),
    }
}
