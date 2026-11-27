//! Finished agents the user put away. The team's roster names its members again on every
//! team event, so the chat remembers what was cleared and does not bring it back; work
//! brings an agent back, and one a request of the user's waits on is never put away.

use super::ChatLog;

impl<L> ChatLog<L> {
    /// Puts away the finished agent `only`, or every finished one, and gives the threads
    /// put away. One at work, or one an approval waits on, stays.
    pub fn clear_agents(&mut self, only: Option<&str>) -> Vec<String> {
        let gone: Vec<String> = self
            .agents
            .iter()
            .filter(|(id, agent)| {
                !agent.working()
                    && only.is_none_or(|only| only == id.as_str())
                    && !self.approval_waits_on(id)
            })
            .map(|(id, _)| id.clone())
            .collect();
        for id in &gone {
            self.agents.remove(id);
            self.agent_chats.remove(id);
            self.cleared.insert(id.clone());
        }
        gone
    }

    /// Keeps agent `thread_id` put away, as a live copy of the chat had it: one the history
    /// does not list is remembered, so a later roster does not bring it back.
    pub(super) fn clear_away(&mut self, thread_id: String) {
        if self.agents.contains_key(&thread_id) {
            self.clear_agents(Some(&thread_id));
        } else {
            self.cleared.insert(thread_id);
        }
    }

    /// The agent was put away and has not worked since.
    pub fn cleared(&self, thread_id: &str) -> bool {
        self.cleared.contains(thread_id)
    }

    /// The agent works again, or asks the user: it is shown again.
    pub(super) fn unclear(&mut self, thread_id: &str) {
        self.cleared.remove(thread_id);
    }
}
