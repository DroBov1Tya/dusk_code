//! The approvals a chat waits on. An agent below the chat, a subagent or a team member at
//! any depth, asks in its own thread; the request is lifted into the chat beside the chat's
//! own, so the user sees and answers it where they are, with who asks.

use super::{ChatLog, Subagent};
use crate::thread::{ThreadEvent, agent_name};
use crate::tools::ApprovalRequest;

/// Who asks when it is the chat's own agent.
pub const CHAT_ASKS: &str = "The agent";

impl<L> ChatLog<L> {
    /// Takes an approval opened or closed by the chat or by any agent below it.
    pub(super) fn follow_approval(&mut self, event: &ThreadEvent) {
        let (asker, event) = event.deepest();
        match event {
            ThreadEvent::Approval(request) => self.open_approval(request.clone(), asker),
            ThreadEvent::ApprovalClosed { id } => self.close_approval(*id),
            _ => {}
        }
    }

    /// A request heard twice, as one a joining client is told again, is listed once.
    pub(super) fn open_approval(&mut self, request: ApprovalRequest, asker: Option<&str>) {
        if let Some(asker) = asker {
            self.asked_by.insert(request.id, asker.to_owned());
        }
        if !self.approvals.iter().any(|open| open.id == request.id) {
            self.approvals.push(request);
        }
    }

    /// The request was answered or its wait ended.
    pub fn close_approval(&mut self, id: u64) {
        self.approvals.retain(|request| request.id != id);
        self.asked_by.remove(&id);
        self.live.close_approval(id);
    }

    /// The thread of the agent below the chat that asks request `id`; none when the chat's
    /// own agent asks.
    pub fn approval_thread(&self, id: u64) -> Option<&str> {
        self.asked_by.get(&id).map(String::as_str)
    }

    /// Who asks request `id`, to put before what it wants: "The agent" for the chat's own,
    /// or the agent's name and what it works as, such as `Ember-0c (code-writer)`.
    pub fn approval_asker(&self, id: u64) -> String {
        let Some(thread_id) = self.approval_thread(id) else {
            return CHAT_ASKS.to_owned();
        };
        match self.find_agent(thread_id) {
            Some(agent) => format!("{} ({})", agent.name, agent.role),
            None => agent_name(thread_id),
        }
    }

    /// The subagent of `thread_id` at any depth below the chat.
    fn find_agent(&self, thread_id: &str) -> Option<&Subagent> {
        self.agents.get(thread_id).or_else(|| {
            self.agent_chats
                .values()
                .find_map(|chat| chat.find_agent(thread_id))
        })
    }

    /// The request a client shows of the open ones, one at a time: `shown` while it still
    /// waits, else the oldest; with its place from 1 and how many wait.
    pub fn shown_approval(&self, shown: Option<u64>) -> Option<(&ApprovalRequest, usize, usize)> {
        let at = shown
            .and_then(|id| self.approvals.iter().position(|open| open.id == id))
            .unwrap_or(0);
        let request = self.approvals.get(at)?;
        Some((request, at + 1, self.approvals.len()))
    }

    /// The request one place after the shown one, or before it, around the ends.
    pub fn step_approval(&self, shown: Option<u64>, forward: bool) -> Option<u64> {
        let (_, place, total) = self.shown_approval(shown)?;
        let at = match forward {
            true => place % total,
            false => (place + total - 2) % total,
        };
        Some(self.approvals[at].id)
    }

    /// Whether an approval of the chat or of an agent below it waits on agent `thread_id`
    /// or on the agents below it.
    pub fn approval_waits_on(&self, thread_id: &str) -> bool {
        self.asked_by.values().any(|asker| {
            asker == thread_id
                || self
                    .agent_chats
                    .get(thread_id)
                    .is_some_and(|chat| chat.find_agent(asker).is_some())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::thread::{WorkKind, activity};
    use crate::tools::Request;

    fn asked(id: u64) -> ApprovalRequest {
        ApprovalRequest {
            id,
            request: Request::Command {
                command: "uv run pytest".into(),
            },
            session: None,
            steps: Vec::new(),
        }
    }

    fn started(thread_id: &str, agent: &str) -> ThreadEvent {
        ThreadEvent::SubagentStarted {
            call_id: format!("call-{thread_id}"),
            thread_id: thread_id.into(),
            agent: agent.into(),
            task: "work".into(),
            effort: None,
            model: None,
            task_id: None,
        }
    }

    fn wrapped(path: &[&str], event: ThreadEvent) -> ThreadEvent {
        path.iter()
            .rev()
            .fold(event, |event, thread_id| ThreadEvent::Subagent {
                thread_id: (*thread_id).into(),
                event: Box::new(event),
            })
    }

    #[test]
    fn an_agents_request_at_any_depth_waits_in_the_chat_named() {
        let mut chat = ChatLog::<std::convert::Infallible>::default();
        chat.apply(started("lead-a", "planner"), 0);
        chat.apply(wrapped(&["lead-a"], started("member-b", "code-writer")), 0);
        chat.apply(ThreadEvent::Approval(asked(1)), 0);
        chat.apply(wrapped(&["lead-a"], ThreadEvent::Approval(asked(2))), 0);
        chat.apply(
            wrapped(&["lead-a", "member-b"], ThreadEvent::Approval(asked(3))),
            0,
        );
        // An agent the chat never heard start still asks, under its thread's name.
        chat.apply(wrapped(&["stray-c"], ThreadEvent::Approval(asked(4))), 0);
        let ids: Vec<u64> = chat.approvals.iter().map(|a| a.id).collect();
        assert_eq!(ids, [1, 2, 3, 4]);
        assert_eq!(chat.approval_asker(1), CHAT_ASKS);
        let lead = format!("{} (planner)", agent_name("lead-a"));
        assert_eq!(chat.approval_asker(2), lead);
        let member = format!("{} (code-writer)", agent_name("member-b"));
        assert_eq!(chat.approval_asker(3), member);
        assert_eq!(chat.approval_asker(4), agent_name("stray-c"));
        assert!(
            chat.approval_waits_on("lead-a"),
            "its member waits below it"
        );

        // Closed from where it was asked, it leaves the chat and the agent's own log.
        chat.apply(
            wrapped(
                &["lead-a", "member-b"],
                ThreadEvent::ApprovalClosed { id: 3 },
            ),
            0,
        );
        chat.apply(
            wrapped(&["lead-a"], ThreadEvent::ApprovalClosed { id: 2 }),
            0,
        );
        let ids: Vec<u64> = chat.approvals.iter().map(|a| a.id).collect();
        assert_eq!(ids, [1, 4]);
        assert!(chat.agent_chats["lead-a"].approvals.is_empty());
        assert!(!chat.approval_waits_on("lead-a"));
    }

    #[test]
    fn approvals_show_one_at_a_time_and_step_around() {
        let mut chat = ChatLog::<std::convert::Infallible>::default();
        assert!(chat.shown_approval(None).is_none());
        for id in [3, 5, 8] {
            chat.apply(ThreadEvent::Approval(asked(id)), 0);
        }
        let place = |chat: &ChatLog, shown| {
            chat.shown_approval(shown)
                .map(|(request, at, total)| (request.id, at, total))
        };
        assert_eq!(place(&chat, None), Some((3, 1, 3)));
        assert_eq!(chat.step_approval(None, true), Some(5));
        assert_eq!(chat.step_approval(Some(3), false), Some(8));
        assert_eq!(chat.step_approval(Some(8), true), Some(3));
        assert_eq!(place(&chat, Some(5)), Some((5, 2, 3)));
        // The shown one answered, the oldest left shows.
        chat.apply(ThreadEvent::ApprovalClosed { id: 5 }, 0);
        assert_eq!(place(&chat, Some(5)), Some((3, 1, 2)));
    }

    #[test]
    fn a_request_told_again_is_listed_once_and_shows_as_waiting() {
        let mut chat = ChatLog::<std::convert::Infallible>::default();
        // A joining client hears an agent's request with no turn of the chat running.
        chat.apply(ThreadEvent::AgentsWorking(true), 0);
        let event = wrapped(&["member-b"], ThreadEvent::Approval(asked(9)));
        chat.apply(event.clone(), 0);
        chat.apply(event, 0);
        assert_eq!(chat.approvals.len(), 1);
        let work = activity(&chat.live, 0).expect("the chat waits");
        assert_eq!(work.kind, WorkKind::WaitingForUser);
        chat.apply(
            wrapped(&["member-b"], ThreadEvent::ApprovalClosed { id: 9 }),
            0,
        );
        assert!(chat.approvals.is_empty());
        let work = activity(&chat.live, 0).expect("its agents still work");
        assert_ne!(work.kind, WorkKind::WaitingForUser);
    }

    #[test]
    fn a_request_answered_here_stops_waiting_before_the_core_closes_it() {
        let mut chat = ChatLog::<std::convert::Infallible>::default();
        chat.apply(ThreadEvent::AgentsWorking(true), 0);
        chat.apply(started("member-b", "code-writer"), 0);
        chat.apply(wrapped(&["member-b"], ThreadEvent::Approval(asked(9))), 0);
        assert!(chat.approval_waits_on("member-b"));
        chat.close_approval(9);
        assert!(chat.approvals.is_empty());
        assert!(!chat.approval_waits_on("member-b"));
        let work = activity(&chat.live, 0).expect("its agents still work");
        assert_ne!(work.kind, WorkKind::WaitingForUser);
    }

    #[test]
    fn a_live_copy_keeps_its_waits_and_clears_over_the_history() {
        let mut history = ChatLog::<std::convert::Infallible>::default();
        history.apply(ThreadEvent::AgentsWorking(true), 0);
        history.apply(started("done-a", "planner"), 0);
        history.apply(wrapped(&["done-a"], ThreadEvent::Idle), 0);
        assert!(!history.agent("done-a").expect("listed").working());
        let mut live = ChatLog::<std::convert::Infallible>::default();
        live.apply(ThreadEvent::AgentsWorking(true), 0);
        live.apply(wrapped(&["member-b"], ThreadEvent::Approval(asked(9))), 0);
        live.cleared.insert("done-a".into());
        live.cleared.insert("gone-c".into());

        history.append_live(live);
        let work = activity(&history.live, 0).expect("the chat waits");
        assert_eq!(work.kind, WorkKind::WaitingForUser);
        assert!(history.agent("done-a").is_none());
        assert!(history.cleared("done-a"));
        assert!(history.cleared("gone-c"));
    }
}
