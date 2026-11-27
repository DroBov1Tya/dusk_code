//! The team that runs a chat's plan, and its members as the chat's agents.

use super::{ChatLog, Subagent};
use crate::thread::{KnownAgent, Team, TeamChange, agent_name, team_call_id};

impl<L> ChatLog<L> {
    /// The team as the core told it by its event. Each member is one of the chat's agents
    /// by the rule every client shares (`Team::changes`): the core announces a member when
    /// it gives it a task, and a member this chat never heard start, as in a chat opened
    /// while its team runs or after, joins in the state of its task.
    pub fn set_team(&mut self, team: Team, now: i64) {
        if self
            .team
            .as_ref()
            .is_some_and(|held| team.older_run_than(held))
        {
            return;
        }
        let known: Vec<KnownAgent<'_>> = self
            .agents
            .values()
            .map(|agent| KnownAgent {
                thread_id: &agent.thread_id,
                task_id: agent.task_id.as_deref(),
                working: agent.working(),
            })
            .collect();
        for change in team.changes(&self.tasks, &known) {
            match change {
                TeamChange::Stop(thread_id) => {
                    if let Some(agent) = self.agents.get_mut(&thread_id) {
                        agent.stop(now);
                    }
                    self.live.agent_stopped(&thread_id);
                }
                TeamChange::Join(member) => {
                    let working = member.state.working();
                    // A member put away stays away until it works again.
                    if self.cleared(&member.thread_id) {
                        if !working {
                            continue;
                        }
                        self.unclear(&member.thread_id);
                    }
                    let agent = Subagent {
                        name: agent_name(&member.thread_id),
                        thread_id: member.thread_id.clone(),
                        role: member.role,
                        model: member.model,
                        effort: None,
                        call_id: team_call_id(&member.task_id),
                        task: member.task,
                        task_id: Some(member.task_id),
                        state: member.state,
                        running: Vec::new(),
                        started_at: now,
                        ended_at: (!working).then_some(now),
                    };
                    self.agents.insert(member.thread_id, agent);
                }
            }
        }
        self.team = Some(team);
    }

    /// The team as a request read it. An answer older than the team the chat holds, one
    /// that left before a newer event came, is dropped.
    pub fn team_answer(&mut self, team: Team, now: i64) {
        if self.team.as_ref().is_none_or(|held| team.replaces(held)) {
            self.set_team(team, now);
        }
    }
}
