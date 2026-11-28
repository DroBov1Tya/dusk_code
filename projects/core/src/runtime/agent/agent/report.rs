//! What the agent tells about itself: its phase, its effort, its tokens, its turn's
//! progress, its events.

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

    /// The chat keeps its own level; a subagent thinks at what its asker chose, from its
    /// chat's level.
    pub(super) async fn effort(&self, config: &Config) -> Option<Effort> {
        let chat = self.model.effort_in(config);
        if self.depth == 0 {
            return chat;
        }
        let asked = match self.state.agent_effort(&self.thread_id).await {
            Ok(asked) => asked,
            Err(error) => {
                warn!("thread {}: {error:#}", self.thread_id);
                None
            }
        };
        Effort::for_subagent(chat, asked)
    }

    /// Counting is for the client to see; a failure here does not touch the thread.
    pub async fn report_tokens(&self) {
        let source = self.context_source();
        let specs = source.specs();
        let counted = async {
            let mut tokens = self.state.thread_tokens(&self.thread_id).await?;
            let stub_results = source.config.current().limits().stub_results;
            let conversation =
                Conversation::load(&self.state, &self.thread_id, stub_results).await?;
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

    /// The turn's totals after one of its model calls, counted here so no client sums
    /// usage. Like the token count, a failure is logged and the turn goes on.
    pub(super) async fn report_progress(&self, turn_id: &str) {
        match self.state.turn_spend(turn_id).await {
            Ok(spend) => self.emit(ThreadEvent::TurnProgress {
                turn_id: turn_id.to_owned(),
                steps: spend.steps,
                tokens: spend.tokens,
                cost_micro_usd: spend.cost_micro_usd,
            }),
            Err(error) => warn!("thread {}: {error:#}", self.thread_id),
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
            scope: self.working_scope(),
            model: Arc::clone(&self.model),
            config: Arc::clone(&self.config),
        }
    }

    pub fn input_limit(&self) -> u64 {
        self.context_source().limit()
    }
}
