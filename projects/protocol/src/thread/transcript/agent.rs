use crate::{model::ModelEvent, thread::ThreadEvent};

/// What a subagent does now, from the events of its thread.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentState {
    Thinking,
    Writing,
    Tool(String),
    Done,
    Failed,
}

impl AgentState {
    pub fn working(&self) -> bool {
        !matches!(self, Self::Done | Self::Failed)
    }

    /// Follows one event of the agent's thread. `running` holds the tools it runs now, by
    /// call ID and name, oldest first: several may run at once, so one that ends hands
    /// over to the newest still running. The model speaks between tools, not over them, so
    /// text and thinking while a tool runs change nothing. An agent that ended works again
    /// only with a new turn: what trails in after a stop must not bring it back. Whether
    /// the state changed.
    pub fn follow(&mut self, running: &mut Vec<(String, String)>, event: &ThreadEvent) -> bool {
        if !self.working() && !matches!(event, ThreadEvent::TurnStarted(_)) {
            return false;
        }
        let state = match event {
            ThreadEvent::TurnStarted(_) => {
                running.clear();
                Self::Thinking
            }
            ThreadEvent::ToolStarted(call) => {
                running.push((call.id.clone(), call.name.clone()));
                Self::Tool(call.name.clone())
            }
            ThreadEvent::ToolFinished { call_id, .. } => {
                running.retain(|(id, _)| id != call_id);
                match running.last() {
                    Some((_, name)) => Self::Tool(name.clone()),
                    None => Self::Thinking,
                }
            }
            ThreadEvent::Model(ModelEvent::Reasoning(_)) if running.is_empty() => Self::Thinking,
            ThreadEvent::Model(ModelEvent::Text(_)) if running.is_empty() => Self::Writing,
            ThreadEvent::Idle => Self::Done,
            ThreadEvent::Failed(_) | ThreadEvent::TurnFailed(_) => Self::Failed,
            _ => return false,
        };
        if !state.working() {
            running.clear();
        }
        let changed = *self != state;
        *self = state;
        changed
    }
}
