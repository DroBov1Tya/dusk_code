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
}
