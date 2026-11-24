//! A team's members as a chat's agents: one rule for every client, whatever list of
//! agents it keeps.

use super::Team;
use crate::thread::{AgentState, Task, TaskStatus};

/// The role a member or task without one works as.
pub const GENERAL_ROLE: &str = "general";

/// `role`, or the general agent's.
pub fn role_name(role: Option<&str>) -> &str {
    role.unwrap_or(GENERAL_ROLE)
}

/// What a client knows of one of its agents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KnownAgent<'a> {
    pub thread_id: &'a str,
    /// The task of the plan it was given; none for an agent a tool call asked.
    pub task_id: Option<&'a str>,
    pub working: bool,
}

/// A member the client did not know on its task, as an agent to add.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Joining {
    pub thread_id: String,
    pub role: String,
    pub model: Option<String>,
    pub task_id: String,
    /// Its task's words, when the plan the client holds has them.
    pub task: String,
    pub state: AgentState,
}

/// What a `Team` changes in a client's agents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TeamChange {
    /// A member it did not know, or one given another task: added, or put in place of
    /// what it knew of that thread.
    Join(Joining),
    /// An agent whose turn the team's state ended: it stops.
    Stop(String),
}

impl Team {
    /// What this team changes in a client's agents. Each member is an agent: one the
    /// client knows on its task keeps its own events, and stops once its task is no longer
    /// at work. A team that ended stops every team member still working, listed or not:
    /// the core gives the tasks at work back without an owner, so their members drop off
    /// the roster it sends then. Members the client learned earlier stay with it.
    pub fn changes(&self, tasks: &[Task], known: &[KnownAgent<'_>]) -> Vec<TeamChange> {
        let ended = self.ended().is_some();
        let mut changes = Vec::new();
        for member in &self.members {
            let at_work = member.status.at_work() && !ended;
            let held = known
                .iter()
                .find(|agent| agent.thread_id == member.thread_id);
            match held {
                Some(agent) if agent.task_id == Some(member.task_id.as_str()) => {
                    if agent.working && !at_work {
                        changes.push(TeamChange::Stop(member.thread_id.clone()));
                    }
                }
                _ => {
                    let state = match member.status {
                        _ if at_work => AgentState::Thinking,
                        TaskStatus::Failed => AgentState::Failed,
                        _ => AgentState::Done,
                    };
                    let task = tasks
                        .iter()
                        .find(|task| task.task_id == member.task_id)
                        .map(|task| task.description.clone())
                        .unwrap_or_default();
                    changes.push(TeamChange::Join(Joining {
                        thread_id: member.thread_id.clone(),
                        role: role_name(member.role.as_deref()).to_owned(),
                        model: member.model.clone(),
                        task_id: member.task_id.clone(),
                        task,
                        state,
                    }));
                }
            }
        }
        if ended {
            let listed = |thread: &str| self.members.iter().any(|m| m.thread_id == thread);
            for agent in known {
                if agent.working && agent.task_id.is_some() && !listed(agent.thread_id) {
                    changes.push(TeamChange::Stop(agent.thread_id.to_owned()));
                }
            }
        }
        changes
    }

    /// Whether this team is of a run older than `held`'s, such as an event of an ended run
    /// replayed after the plan ran again: it changes nothing, and above all stops none of
    /// the new run's members. Unknown when either run is not told (an older core).
    pub fn older_run_than(&self, held: &Team) -> bool {
        matches!((self.started_at, held.started_at), (Some(mine), Some(theirs)) if mine < theirs)
    }

    /// Whether this team, read by a request, may take the place of `held`, which the
    /// chat holds from an event or an earlier answer. An answer that was on its way
    /// while a newer event came is older: it has used no more, or it says running of a
    /// team that ended.
    pub fn replaces(&self, held: &Team) -> bool {
        // Another run of the same chat's team: the later run wins, whatever its state.
        if let (Some(mine), Some(theirs)) = (self.started_at, held.started_at)
            && mine != theirs
        {
            return mine > theirs;
        }
        match (self.ended(), held.ended()) {
            (None, Some(_)) => false,
            (Some(_), None) => true,
            _ => {
                let used = |team: &Team| (team.used.minutes, team.used.steps, team.used.tokens);
                used(self) >= used(held)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::thread::{TeamBudget, TeamEnd, TeamMember, TeamStatus, TeamUsed};

    fn team(status: TeamStatus, members: &[(&str, &str, TaskStatus)]) -> Team {
        Team {
            thread_id: "lead".into(),
            status,
            budget: TeamBudget::new(2, 100, 1_000_000, 60).unwrap(),
            used: TeamUsed::default(),
            members: members
                .iter()
                .map(|(thread, task, status)| TeamMember {
                    thread_id: (*thread).into(),
                    name: (*thread).into(),
                    role: None,
                    model: None,
                    task_id: (*task).into(),
                    status: *status,
                })
                .collect(),
            started_at: None,
            ended_at: None,
        }
    }

    fn known<'a>(thread: &'a str, task: Option<&'a str>, working: bool) -> KnownAgent<'a> {
        KnownAgent {
            thread_id: thread,
            task_id: task,
            working,
        }
    }

    /// The core sends an ended team without the members whose tasks went back: they are
    /// stopped all the same, and an agent no team call started is left alone.
    #[test]
    fn an_ended_team_stops_its_members_off_the_roster_too() {
        let ended = team(
            TeamStatus::Ended(TeamEnd::Stopped),
            &[("m1", "t1", TaskStatus::Done)],
        );
        let agents = [
            known("m1", Some("t1"), false),
            known("m2", Some("t2"), true),
            known("asked", None, true),
        ];
        assert_eq!(ended.changes(&[], &agents), [TeamChange::Stop("m2".into())]);
    }

    #[test]
    fn an_older_answer_does_not_replace_a_newer_team() {
        let mut running = team(TeamStatus::Running, &[]);
        running.used.minutes = 5;
        let mut ended = team(TeamStatus::Ended(TeamEnd::Stopped), &[]);
        ended.used.minutes = 5;
        assert!(!running.replaces(&ended));
        assert!(ended.replaces(&running));
        let mut later = running.clone();
        later.used.steps = 3;
        assert!(later.replaces(&running) && !running.replaces(&later));
    }

    /// Running the plan again starts a new run of the same team: the later run wins both
    /// ways, whatever either says or used, and a team from a core that tells no runs falls
    /// back to the rule within a run.
    #[test]
    fn a_later_run_replaces_an_earlier_one_and_never_the_reverse() {
        let run = |started: i64, status: TeamStatus, minutes: u32| {
            let mut team = team(status, &[]);
            team.started_at = Some(started);
            team.used.minutes = minutes;
            team
        };
        let first_ended = run(100, TeamStatus::Ended(TeamEnd::Steps), 57);
        let second_running = run(200, TeamStatus::Running, 1);
        let second_ended = run(200, TeamStatus::Ended(TeamEnd::Done), 2);
        // An old ended answer after the new run's event is refused.
        assert!(!first_ended.replaces(&second_running));
        assert!(first_ended.older_run_than(&second_running));
        // The new run's answer replaces the old ended team, running or ended.
        assert!(second_running.replaces(&first_ended));
        assert!(second_ended.replaces(&first_ended));
        assert!(!second_running.older_run_than(&first_ended));
        // Within one run the earlier rule holds.
        assert!(!second_running.replaces(&second_ended));
        let mut unknown = second_running.clone();
        unknown.started_at = None;
        assert!(!unknown.replaces(&first_ended) && !unknown.older_run_than(&first_ended));
    }
}
