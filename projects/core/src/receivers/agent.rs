use anyhow::{Context, Result, ensure};

use crate::state::{Message, MessageAttachment, State, Turn};

const CONVERSATION_PAGE: u32 = 1000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentOutcome {
    Completed {
        content: String,
        attachments: Vec<MessageAttachment>,
    },
    Failed {
        error_message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentResponse {
    pub turn_id: String,
    pub expected_attempt: u64,
    pub outcome: AgentOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentReceipt {
    pub turn: Turn,
    pub message: Option<Message>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentTask {
    pub turn: Turn,
    pub user_message: Message,
}

/// Internal capability handed to a trusted agent runtime, never to the public adapter.
/// The host authenticates the runtime before binding its identity here.
pub struct AgentReceiver<'a> {
    state: &'a State,
    agent_id: String,
}

impl<'a> AgentReceiver<'a> {
    pub fn new(state: &'a State, agent_id: impl Into<String>) -> Result<Self> {
        let agent_id = agent_id.into();
        ensure!(!agent_id.trim().is_empty(), "agent ID is empty");
        Ok(Self { state, agent_id })
    }

    /// The runtime can recover queued work after missing a live notification.
    pub async fn pending_turns(&self, thread_id: &str, limit: u32) -> Result<Vec<Turn>> {
        self.state.pending_turns(thread_id, limit).await
    }

    pub async fn start_turn(&self, turn_id: &str, expected_attempt: u64) -> Result<AgentTask> {
        let turn = self
            .state
            .start_turn(turn_id, &self.agent_id, expected_attempt)
            .await?;
        let user_message = self
            .state
            .load_message(&turn.user_message_id)
            .await?
            .context("turn user message does not exist")?;
        Ok(AgentTask { turn, user_message })
    }

    /// Thread messages up to and including the task's user message, in order.
    pub async fn conversation(&self, task: &AgentTask) -> Result<Vec<Message>> {
        let last = task.user_message.sequence;
        let mut messages = Vec::new();
        let mut after = 0;
        while after < last {
            let page = self
                .state
                .list_messages(&task.turn.thread_id, after, CONVERSATION_PAGE)
                .await?;
            let Some(tail) = page.last() else { break };
            after = tail.sequence;
            messages.extend(page.into_iter().filter(|message| message.sequence <= last));
        }
        Ok(messages)
    }

    /// Core chooses the lifecycle transition and message role from the result type.
    pub async fn submit_response(&self, response: AgentResponse) -> Result<AgentReceipt> {
        match response.outcome {
            AgentOutcome::Completed {
                content,
                attachments,
            } => {
                let (turn, message) = self
                    .state
                    .complete_turn(
                        &response.turn_id,
                        &self.agent_id,
                        response.expected_attempt,
                        content,
                        attachments,
                    )
                    .await?;
                Ok(AgentReceipt {
                    turn,
                    message: Some(message),
                })
            }
            AgentOutcome::Failed { error_message } => {
                let turn = self
                    .state
                    .fail_turn(
                        &response.turn_id,
                        &self.agent_id,
                        response.expected_attempt,
                        error_message,
                    )
                    .await?;
                Ok(AgentReceipt {
                    turn,
                    message: None,
                })
            }
        }
    }
}
