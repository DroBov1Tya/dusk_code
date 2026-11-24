//! One selection for both layers: point at a member and its task lights in Plan with
//! what it waits on; point at a task and its owner lights in People with what the task
//! needs. Everything else dims.

use std::collections::HashMap;

use super::Ends;
use crate::thread::{Task, TaskStatus, Team, WaitsOn};

/// What the pointer is on, or what was clicked.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Pick {
    /// A member, by its thread.
    Member(String),
    /// A task, by its id.
    Task(String),
}

/// How a node stands to the selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lit {
    /// The node picked, or its other half: a member's task, a task's owner.
    Picked,
    /// What it needs or waits on.
    Related,
    Dim,
}

/// The nodes a pick lights, by member thread and by task id.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Related {
    members: HashMap<String, Lit>,
    tasks: HashMap<String, Lit>,
}

impl Related {
    /// What `pick` lights in `team` and its plan `tasks`; none when nothing is picked or
    /// the pick is gone, and then nothing dims.
    pub fn of(pick: Option<&Pick>, team: Option<&Team>, tasks: &[Task]) -> Option<Self> {
        let task = |id: &str| tasks.iter().find(|task| task.task_id == id);
        let mut related = Self::default();
        match pick? {
            Pick::Member(thread) => {
                let member = team?.members.iter().find(|m| m.thread_id == *thread)?;
                related.members.insert(thread.clone(), Lit::Picked);
                related.tasks.insert(member.task_id.clone(), Lit::Picked);
                if let Some(Task {
                    waits_on: Some(WaitsOn::Task(on)),
                    ..
                }) = task(&member.task_id)
                {
                    related.relate_task(on, task(on));
                }
            }
            Pick::Task(id) => {
                let picked = task(id)?;
                related.tasks.insert(id.clone(), Lit::Picked);
                if let Some(owner) = &picked.owner {
                    related.members.insert(owner.thread_id.clone(), Lit::Picked);
                }
                let waits = match &picked.waits_on {
                    Some(WaitsOn::Task(on)) => Some(on),
                    _ => None,
                };
                for need in picked.depends_on.iter().chain(waits) {
                    related.relate_task(need, task(need));
                }
            }
        }
        Some(related)
    }

    /// Lights `id` as related, and its owner while it is not done.
    fn relate_task(&mut self, id: &str, task: Option<&Task>) {
        self.tasks.entry(id.to_owned()).or_insert(Lit::Related);
        let owner = task
            .filter(|task| task.status != TaskStatus::Done)
            .and_then(|task| task.owner.as_ref());
        if let Some(owner) = owner {
            self.members
                .entry(owner.thread_id.clone())
                .or_insert(Lit::Related);
        }
    }

    pub fn member(&self, thread_id: &str) -> Lit {
        self.members.get(thread_id).copied().unwrap_or(Lit::Dim)
    }

    pub fn task(&self, task_id: &str) -> Lit {
        self.tasks.get(task_id).copied().unwrap_or(Lit::Dim)
    }

    /// Whether an edge lights: one to a lit member, an arc between two, or a need of the
    /// task picked.
    pub fn edge(&self, ends: &Ends) -> bool {
        match ends {
            Ends::Lead(member) => self.members.contains_key(member),
            Ends::Members(a, b) => self.members.contains_key(a) && self.members.contains_key(b),
            Ends::Tasks(a, b) => {
                let (a, b) = (self.task(a), self.task(b));
                a != Lit::Dim && b != Lit::Dim && (a == Lit::Picked || b == Lit::Picked)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::thread::{TaskOwner, TeamMember, TeamStatus, TeamUsed};

    fn task(id: &str, needs: &[&str], status: TaskStatus, owner: Option<&str>) -> Task {
        Task {
            task_id: id.into(),
            status,
            done: status == TaskStatus::Done,
            depends_on: needs.iter().map(|need| (*need).into()).collect(),
            owner: owner.map(|owner| TaskOwner {
                thread_id: owner.into(),
                name: owner.into(),
            }),
            ..Task::default()
        }
    }

    #[test]
    fn a_member_and_a_task_light_each_other_and_what_they_wait_on() {
        let mut waiting = task("t3", &["t1"], TaskStatus::Blocked, Some("m3"));
        waiting.waits_on = Some(WaitsOn::Task("t2".into()));
        let tasks = [
            task("t1", &[], TaskStatus::Done, Some("m1")),
            task("t2", &[], TaskStatus::Running, Some("m2")),
            waiting,
        ];
        let team = Team {
            thread_id: "lead".into(),
            started_at: None,
            winding_since: None,
            goal: String::new(),
            status: TeamStatus::Running,
            at_once: 3,
            used: TeamUsed::default(),
            members: ["m1", "m2", "m3"]
                .iter()
                .zip(["t1", "t2", "t3"])
                .map(|(member, task)| TeamMember {
                    thread_id: (*member).into(),
                    name: (*member).into(),
                    role: None,
                    model: None,
                    task_id: task.into(),
                    status: TaskStatus::Running,
                })
                .collect(),
        };
        let member = Related::of(Some(&Pick::Member("m3".into())), Some(&team), &tasks).unwrap();
        assert_eq!(member.task("t3"), Lit::Picked);
        assert_eq!(member.task("t2"), Lit::Related);
        assert_eq!(member.member("m2"), Lit::Related);
        assert_eq!(member.member("m1"), Lit::Dim);
        assert!(member.edge(&Ends::Members("m3".into(), "m2".into())));

        let picked = Related::of(Some(&Pick::Task("t3".into())), Some(&team), &tasks).unwrap();
        assert_eq!(picked.member("m3"), Lit::Picked);
        assert_eq!(picked.task("t1"), Lit::Related);
        // A done need's owner stays dim; a running one's lights.
        assert_eq!(picked.member("m1"), Lit::Dim);
        assert_eq!(picked.member("m2"), Lit::Related);
        assert!(picked.edge(&Ends::Tasks("t1".into(), "t3".into())));
        assert!(!picked.edge(&Ends::Tasks("t1".into(), "t2".into())));

        assert!(Related::of(None, Some(&team), &tasks).is_none());
        assert!(Related::of(Some(&Pick::Task("gone".into())), Some(&team), &tasks).is_none());
    }
}
