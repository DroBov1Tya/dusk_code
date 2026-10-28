//! The agent runtime's entry point: claims queued turns and records their outcome.

use anyhow::{Context, Result, ensure};

use crate::state::{Message, MessageAttachment, State, Turn};

const CONVERSATION_PAGE: u32 = 1000;

/// Internal capability handed to a trusted agent runtime, never to the public adapter.
/// The host authenticates the runtime before binding its identity here.
pub struct AgentReceiver<'a> {
    state: &'a State,
    agent_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentTask {
    pub turn: Turn,
    pub user_message: Message,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentResponse {
    pub turn_id: String,
    pub expected_attempt: u64,
    pub outcome: AgentOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum AgentOutcome {
    Completed {
        content: String,
        /// Stored only when the session records reasoning.
        reasoning: Option<String>,
        attachments: Vec<MessageAttachment>,
    },
    Failed {
        error_message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentReceipt {
    pub turn: Turn,
    pub message: Option<Message>,
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
                reasoning,
                attachments,
            } => {
                let (turn, message) = self
                    .state
                    .complete_turn(
                        &response.turn_id,
                        &self.agent_id,
                        response.expected_attempt,
                        content,
                        reasoning,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        receivers::{UserMessage, UserReceiver},
        state::{ApprovalMode, MessageRole, ThreadInput},
        testing::temp_state,
    };

    async fn queued_turn(state: &State, uid: &str) -> Turn {
        if state.load_thread("thread").await.unwrap().is_none() {
            let thread = ThreadInput {
                source: "test".into(),
                tokens_used: 0,
                compaction_count: 0,
                sandbox_policy: "none".into(),
                approval_mode: ApprovalMode::Never,
                git_branch: None,
                git_commit_sha: None,
                git_origin_url: None,
                title: String::new(),
            };
            state.create_thread("thread", thread).await.unwrap();
        }
        let message = UserMessage {
            request_uid: uid.into(),
            thread_id: "thread".into(),
            content: "question".into(),
            cwd: "/tmp".into(),
            attachments: Vec::new(),
        };
        let receipt = UserReceiver::new(state).submit_message(message).await;
        receipt.unwrap().turn.unwrap()
    }

    async fn answer(agent: &AgentReceiver<'_>, turn: &Turn, reasoning: Option<&str>) -> Message {
        let task = agent.start_turn(&turn.turn_id, turn.attempt).await.unwrap();
        let response = AgentResponse {
            turn_id: task.turn.turn_id,
            expected_attempt: task.turn.attempt,
            outcome: AgentOutcome::Completed {
                content: "answer".into(),
                reasoning: reasoning.map(Into::into),
                attachments: Vec::new(),
            },
        };
        agent
            .submit_response(response)
            .await
            .unwrap()
            .message
            .unwrap()
    }

    #[tokio::test]
    async fn stores_reasoning_only_when_given() {
        let state = temp_state("agent").await;
        let agent = AgentReceiver::new(&state, "agent").unwrap();

        let full = answer(&agent, &queued_turn(&state, "r1").await, Some("think")).await;
        let short = answer(&agent, &queued_turn(&state, "r2").await, None).await;

        let stored = state.list_messages("thread", 0, 10).await.unwrap();
        let assistant: Vec<_> = stored
            .iter()
            .filter(|message| message.role == MessageRole::Assistant)
            .collect();
        assert_eq!(assistant, [&full, &short]);
        assert_eq!(full.reasoning.as_deref(), Some("think"));
        assert_eq!(short.reasoning, None);
        assert!(
            stored
                .iter()
                .all(|message| message.role != MessageRole::User || message.reasoning.is_none())
        );
        state.close().await;
    }

    #[tokio::test]
    async fn conversation_ends_at_the_task_message() {
        let state = temp_state("agent").await;
        let agent = AgentReceiver::new(&state, "agent").unwrap();
        answer(&agent, &queued_turn(&state, "r1").await, None).await;
        let second = queued_turn(&state, "r2").await;
        queued_turn(&state, "r3").await;

        let task = agent
            .start_turn(&second.turn_id, second.attempt)
            .await
            .unwrap();
        let history = agent.conversation(&task).await.unwrap();
        assert_eq!(history.len(), 3);
        assert_eq!(
            history.last().unwrap().message_id,
            task.user_message.message_id
        );
        state.close().await;
    }
}
