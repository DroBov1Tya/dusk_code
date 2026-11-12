//! What the agent tells about itself: its phase, its effort, its tokens, its events.

use super::Agent;
use crate::runtime::{
    ThreadEvent,
    thread::{ContextSource, Conversation, spec_tokens},
};
use crate::{
    config::{Config, Effort},
    state::AgentPhase,
};
use std::sync::Arc;
use tracing::warn;

impl Agent {
    /// A subagent's phase; a chat has none.
    pub(super) async fn phase(&self) -> Option<AgentPhase> {
        if self.depth == 0 {
            return None;
        }
        match self.state.agent_phase(&self.thread_id).await {
            Ok(phase) => phase,
            Err(error) => {
                warn!("thread {}: {error:#}", self.thread_id);
                None
            }
        }
    }

    /// The runtime's own move of a subagent's phase. A failure to record it is logged:
    /// the phase is for people to follow and must not stop the work.
    pub(super) async fn runtime_phase(&self, phase: AgentPhase) {
        if self.depth == 0 {
            return;
        }
        if let Err(error) = self.state.set_agent_phase(&self.thread_id, phase, "").await {
            warn!("thread {}: {error:#}", self.thread_id);
            return;
        }
        self.emit(ThreadEvent::AgentPhase {
            phase,
            note: String::new(),
        });
    }

    /// The chat keeps the user's level; a subagent thinks at what its asker chose.
    pub(super) async fn effort(&self, config: &Config) -> Option<Effort> {
        if self.depth == 0 {
            return config.effort();
        }
        let asked = match self.state.agent_effort(&self.thread_id).await {
            Ok(asked) => asked,
            Err(error) => {
                warn!("thread {}: {error:#}", self.thread_id);
                None
            }
        };
        Effort::for_subagent(config.effort(), asked)
    }

    /// Counting is for the client to see; a failure here does not touch the thread.
    pub async fn report_tokens(&self) {
        let source = self.context_source();
        let specs = source.specs();
        let counted = async {
            let mut tokens = self.state.thread_tokens(&self.thread_id).await?;
            let conversation = Conversation::load(&self.state, &self.thread_id).await?;
            tokens.current.tool_specs = spec_tokens(&specs);
            tokens.context = conversation.tokens(&specs);
            anyhow::Ok(tokens)
        };
        match counted.await {
            Ok(tokens) => self.emit(ThreadEvent::Tokens {
                tokens,
                limit: source.limit(),
            }),
            Err(error) => warn!("cannot count thread tokens: {error:#}"),
        }
    }

    pub fn emit(&self, event: ThreadEvent) {
        // A client that stopped listening does not stop the thread.
        let _ = self.events.send(event);
    }

    /// What this thread's context is counted with; its mailbox shares the same.
    pub fn context_source(&self) -> ContextSource {
        ContextSource {
            tools: Arc::clone(&self.tools),
            scope: Arc::clone(&self.scope),
            model: Arc::clone(&self.model),
            config: Arc::clone(&self.config),
        }
    }

    pub fn input_limit(&self) -> u64 {
        self.context_source().limit()
    }
}
