//! The team that runs a chat's plan, and its members as the chat's agents.

use super::{ChatLog, Subagent};
use crate::thread::{AgentState, TaskStatus, Team, agent_name, team_call_id};

impl<L> ChatLog<L> {
    /// The team as the core told it, by its event or the answer to `Team`. Each member
    /// is one of the chat's agents: the core announces a member when it gives it a task,
    /// and a member this chat never heard start, as in a chat opened while its team
    /// runs or after, joins in the state of its task.
    pub fn set_team(&mut self, team: Team, now: i64) {
        for member in &team.members {
            let at_work = matches!(member.status, TaskStatus::Claimed | TaskStatus::Running);
            match self.agents.get_mut(&member.thread_id) {
                Some(agent) if agent.task_id.as_deref() == Some(member.task_id.as_str()) => {
                    // Its own events lead; a task no longer at work ended its turn.
                    if agent.working() && !at_work {
                        agent.stop(now);
                    }
                }
                _ => {
                    let state = match member.status {
                        _ if at_work => AgentState::Thinking,
                        TaskStatus::Failed => AgentState::Failed,
                        _ => AgentState::Done,
                    };
                    let task = self
                        .tasks
                        .iter()
                        .find(|task| task.task_id == member.task_id)
                        .map(|task| task.description.clone())
                        .unwrap_or_default();
                    let agent = Subagent {
                        thread_id: member.thread_id.clone(),
                        name: agent_name(&member.thread_id),
                        role: member.role.clone().unwrap_or_else(|| "general".into()),
                        model: member.model.clone(),
                        call_id: team_call_id(&member.task_id),
                        task,
                        task_id: Some(member.task_id.clone()),
                        state,
                        running: Vec::new(),
                        started_at: now,
                        ended_at: (!at_work).then_some(now),
                    };
                    self.agents.insert(member.thread_id.clone(), agent);
                }
            }
        }
        self.team = Some(team);
    }
}
