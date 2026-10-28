//! The agent runtime's entry point: reads a thread, runs turns over its queued messages
//! and records their outcome.

use anyhow::{Result, ensure};
use uuid::Uuid;

use crate::state::{Message, MessageRole, State, Turn, TurnAnswer, TurnOutcome};

const CONVERSATION_PAGE: u32 = 1000;

pub struct AgentReceiver<'a> {
    state: &'a State,
    agent_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conversation {
    pub messages: Vec<Message>,
    /// Sequence of the newest message a turn has taken; user messages after it are queued.
    pub answered_through: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum AgentOutcome {
    Completed {
        content: String,
        reasoning: Option<String>,
    },
    /// Stopped early; `content` is whatever the agent had written, possibly nothing.
    Interrupted {
        content: String,
        reasoning: Option<String>,
    },
    Failed {
        error_message: String,
    },
}

impl Conversation {
    pub fn has_queued(&self) -> bool {
        self.messages.iter().any(|message| {
            message.role == MessageRole::User && message.sequence > self.answered_through
        })
    }
}

impl<'a> AgentReceiver<'a> {
    pub fn new(state: &'a State, agent_id: impl Into<String>) -> Result<Self> {
        let agent_id = agent_id.into();
        ensure!(!agent_id.trim().is_empty(), "agent ID is empty");
        Ok(Self { state, agent_id })
    }

    /// Every message of the thread in order, queued ones included.
    pub async fn conversation(&self, thread_id: &str) -> Result<Conversation> {
        let answered_through = self.state.answered_through(thread_id).await?;
        let mut messages = Vec::new();
        loop {
            let after = messages
                .last()
                .map_or(0, |message: &Message| message.sequence);
            let page = self
                .state
                .list_messages(thread_id, after, CONVERSATION_PAGE)
                .await?;
            if page.is_empty() {
                return Ok(Conversation {
                    messages,
                    answered_through,
                });
            }
            messages.extend(page);
        }
    }

    /// Starts a turn over every queued user message; `None` when nothing is queued.
    pub async fn start_turn(&self, thread_id: &str) -> Result<Option<Turn>> {
        let turn_id = Uuid::now_v7().to_string();
        self.state.start_turn(thread_id, &turn_id).await
    }

    pub async fn finish_turn(&self, turn: &Turn, outcome: AgentOutcome) -> Result<Turn> {
        let outcome = match outcome {
            AgentOutcome::Completed { content, reasoning } => {
                TurnOutcome::Completed(self.answer(content, reasoning))
            }
            AgentOutcome::Interrupted { content, reasoning } => {
                let written = !content.trim().is_empty();
                TurnOutcome::Interrupted(written.then(|| self.answer(content, reasoning)))
            }
            AgentOutcome::Failed { error_message } => TurnOutcome::Failed(error_message),
        };
        let (turn, _) = self.state.finish_turn(&turn.turn_id, outcome).await?;
        Ok(turn)
    }

    /// Marks turns left running by a stopped process as interrupted.
    pub async fn recover(&self, thread_id: &str) -> Result<()> {
        self.state.interrupt_running_turns(thread_id).await
    }

    fn answer(&self, content: String, reasoning: Option<String>) -> TurnAnswer {
        TurnAnswer {
            content,
            reasoning,
            agent_id: self.agent_id.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        receivers::{UserMessage, UserReceiver},
        state::{ApprovalMode, ThreadInput, TurnStatus},
        testing::temp_state,
    };

    async fn thread(state: &State) {
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

    async fn send(state: &State, uid: &str) {
        let message = UserMessage {
            request_uid: uid.into(),
            thread_id: "thread".into(),
            content: format!("message {uid}"),
            cwd: "/tmp".into(),
            attachments: Vec::new(),
        };
        let receipt = UserReceiver::new(state).submit_message(message).await;
        assert!(receipt.unwrap().message.is_some());
    }

    fn completed(content: &str, reasoning: Option<&str>) -> AgentOutcome {
        AgentOutcome::Completed {
            content: content.into(),
            reasoning: reasoning.map(Into::into),
        }
    }

    #[tokio::test]
    async fn one_turn_answers_every_queued_message() {
        let state = temp_state("batch").await;
        thread(&state).await;
        let agent = AgentReceiver::new(&state, "agent").unwrap();
        assert_eq!(agent.start_turn("thread").await.unwrap(), None);

        send(&state, "a").await;
        send(&state, "b").await;
        let turn = agent.start_turn("thread").await.unwrap().unwrap();
        assert_eq!(turn.last_sequence, 2);
        assert_eq!(agent.start_turn("thread").await.unwrap(), None);
        send(&state, "c").await;
        assert!(
            agent.start_turn("thread").await.is_err(),
            "one running turn"
        );

        let turn = agent
            .finish_turn(&turn, completed("answer", None))
            .await
            .unwrap();
        assert_eq!(turn.status, TurnStatus::Completed);
        let next = agent.start_turn("thread").await.unwrap().unwrap();
        assert_eq!(next.last_sequence, 3, "c was queued during the first turn");
        state.close().await;
    }

    #[tokio::test]
    async fn stores_reasoning_only_when_given() {
        let state = temp_state("reasoning").await;
        thread(&state).await;
        let agent = AgentReceiver::new(&state, "agent").unwrap();
        for (uid, reasoning) in [("a", Some("think")), ("b", None)] {
            send(&state, uid).await;
            let turn = agent.start_turn("thread").await.unwrap().unwrap();
            agent
                .finish_turn(&turn, completed("answer", reasoning))
                .await
                .unwrap();
        }

        let conversation = agent.conversation("thread").await.unwrap();
        let reasoning: Vec<_> = conversation
            .messages
            .iter()
            .filter(|message| message.role == MessageRole::Assistant)
            .map(|message| message.reasoning.as_deref())
            .collect();
        assert_eq!(reasoning, [Some("think"), None]);
        state.close().await;
    }

    #[tokio::test]
    async fn interruption_keeps_the_partial_answer() {
        let state = temp_state("interrupt").await;
        thread(&state).await;
        let agent = AgentReceiver::new(&state, "agent").unwrap();
        send(&state, "a").await;
        let turn = agent.start_turn("thread").await.unwrap().unwrap();
        let partial = AgentOutcome::Interrupted {
            content: "half an ans".into(),
            reasoning: None,
        };
        let turn = agent.finish_turn(&turn, partial).await.unwrap();

        assert_eq!(turn.status, TurnStatus::Interrupted);
        let conversation = agent.conversation("thread").await.unwrap();
        assert_eq!(conversation.messages.last().unwrap().content, "half an ans");
        assert!(!conversation.has_queued());
        state.close().await;
    }

    #[tokio::test]
    async fn recover_interrupts_a_turn_left_running() {
        let state = temp_state("recover").await;
        thread(&state).await;
        let agent = AgentReceiver::new(&state, "agent").unwrap();
        send(&state, "a").await;
        let turn = agent.start_turn("thread").await.unwrap().unwrap();

        agent.recover("thread").await.unwrap();

        let turn = state.load_turn(&turn.turn_id).await.unwrap().unwrap();
        assert_eq!(turn.status, TurnStatus::Interrupted);
        send(&state, "b").await;
        assert!(agent.start_turn("thread").await.unwrap().is_some());
        state.close().await;
    }
}
