//! `/goal [text | pause | resume | clear | done]`: sets the thread's standing goal, or
//! changes it. Setting or resuming queues a message, so the work starts at once.

use crate::{
    chat::goal::{self, GoalAction},
    runtime::{ThreadEvent, agent::Agent},
    state::GoalStatus,
};

pub async fn run(agent: &Agent, argument: Option<&str>) {
    let action = GoalAction::from_command(argument);
    let starts = action.starts_work();
    let applied = async {
        let (goal, note) = goal::apply(&agent.state, &agent.thread_id, action).await?;
        let mut started = None;
        if starts
            && let Some(goal) = goal
                .as_ref()
                .filter(|goal| goal.status == GoalStatus::Active)
        {
            let rounds = agent.config.current().limits().max_goal_rounds;
            let kickoff = goal::kickoff(goal, rounds);
            agent.state.add_pending(&agent.thread_id, &kickoff).await?;
            started = Some(kickoff);
        }
        anyhow::Ok((goal, note, started))
    };
    match applied.await {
        Ok((goal, note, started)) => agent.emit(ThreadEvent::Goal {
            goal,
            note,
            started,
        }),
        Err(error) => agent.emit(ThreadEvent::Rejected {
            reason: format!("{error:#}"),
        }),
    }
}
