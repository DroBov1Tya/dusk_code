//! Putting old tool results away (`limits.stub_results`): which stored results are sent as
//! a one-line stub from now on, and when a move pays for the part of the provider's cached
//! prefix it breaks. A move happens before a request is built, writes all its stubs at
//! once, and never changes a message at or before the newest stub, so each later request
//! extends the one before. With the switch off nothing here runs, reads or writes.

#[cfg(test)]
mod tests;
mod text;

use std::collections::HashMap;

use anyhow::Result;
use dusk_protocol::system::now_ms;
use serde_json::Value;
use tracing::warn;

use super::Agent;
use crate::{
    config::{Config, Limits, ProviderKind},
    runtime::{ThreadEvent, thread::Conversation},
    state::{CallEnd, Message, MessageRole, NewStub, calls_of, saved_tokens},
};
pub(super) use text::left_out_note;
use text::{Stored, index_line, stub_text};

/// The tools whose results are put away: they hold nearly all tool tokens, and what they
/// did fits in a line. Questions, agents, plans, goals, MCP tools and the rest never are.
const STUBBED_TOOLS: [&str; 4] = ["run_command", "read_file", "search_files", "read_history"];
/// A result this large is old after `BIG_KEEP_STEPS` steps: carrying it costs more than
/// reading it back.
const BIG_RESULT_TOKENS: u64 = 4_000;
const BIG_KEEP_STEPS: usize = 2;
/// The least a move must free to be worth breaking the cache.
const MIN_SAVED_TOKENS: u64 = 4_000;
/// On a warm cache a move must free at least 3/5 (0.6) of the tokens that stay after its
/// first stub, which then pays back within about 15 steps at a cached read of 0.1.
const WARM_SHARE: (u64, u64) = (3, 5);
/// After this long without a model call the provider's cache is taken to be gone.
const COLD_AFTER_MS: i64 = 300_000;
/// Results a compaction's index lists at most, the newest it covers.
const INDEX_LINES: usize = 30;
/// Heads the index; the lines after it are built by the core alone.
const INDEX_INTRO: &str =
    "Stored results, newest last (read_history {\"result\": N} returns one whole):";

/// A stored result old enough to put away, and the call it answers.
struct Candidate<'a> {
    result: &'a Message,
    tool: &'static str,
    arguments: &'a Value,
}

/// What a move writes: its stubs, the tokens they free (S) and the tokens of the request
/// from its first stub on that stay (R).
struct Move {
    stubs: Vec<NewStub>,
    saved: u64,
    remaining: u64,
}

/// Whether a move waits until it pays, or happens now.
enum When<'a> {
    ItPays(&'a Config),
    /// After a compaction the context is read cold anyway, so the newest stub and the
    /// thresholds do not hold it back.
    Now,
}

impl Agent {
    /// Before a step: puts old results away when it pays, so this step's request already
    /// sends their stubs. A move that fails is logged, and the step goes on with the
    /// context as it was.
    pub(super) async fn put_away_old_results(
        &self,
        conversation: &mut Conversation,
        config: &Config,
    ) {
        let limits = config.limits();
        if !limits.stub_results {
            return;
        }
        let moved = self.put_away(conversation, &limits, When::ItPays(config));
        if let Err(error) = moved.await {
            warn!(
                "thread {}: old results stay whole: {error:#}",
                self.thread_id
            );
        }
    }

    /// Puts away every old result of `conversation` now, with no thresholds, and updates
    /// its stubs: for the context after a compaction, which is read cold anyway.
    /// Nothing happens with `limits.stub_results` off.
    pub async fn put_away_all_results(
        &self,
        conversation: &mut Conversation,
        limits: &Limits,
    ) -> Result<()> {
        if !limits.stub_results {
            return Ok(());
        }
        self.put_away(conversation, limits, When::Now).await
    }

    /// The index a compaction's summary through `through` ends with: a line for each of
    /// the newest results it covers that are large enough to put away, so the model still
    /// knows what it can get back. Built from stored data only; empty when there are none.
    pub async fn stored_results_index(
        &self,
        conversation: &Conversation,
        through: u64,
        limits: &Limits,
    ) -> Result<String> {
        let calls = calls_of(&conversation.messages);
        let mut covered: Vec<_> = conversation
            .messages
            .iter()
            .filter(|message| message.role == MessageRole::Tool && message.sequence <= through)
            .filter(|message| message.content_tokens >= limits.stub_min_tokens)
            .filter_map(|result| Some((result, calls.get(result.message_id.as_str())?.1)))
            .collect();
        let newest = covered.split_off(covered.len().saturating_sub(INDEX_LINES));
        let Some((first, _)) = newest.first() else {
            return Ok(String::new());
        };
        let ends = self
            .state
            .call_ends(&self.thread_id, first.sequence)
            .await?;
        let lines: Vec<String> = newest
            .iter()
            .map(|(result, call)| {
                index_line(&Stored {
                    sequence: result.sequence,
                    tool: &call.name,
                    arguments: &call.arguments,
                    end: ends.get(&result.message_id).copied(),
                    content: &result.content,
                    tokens: result.content_tokens,
                })
            })
            .collect();
        Ok(format!("\n\n{INDEX_INTRO}\n{}", lines.join("\n")))
    }

    async fn put_away(
        &self,
        conversation: &mut Conversation,
        limits: &Limits,
        when: When<'_>,
    ) -> Result<()> {
        let after = match when {
            When::ItPays(_) => frontier(conversation),
            When::Now => 0,
        };
        let candidates = candidates(conversation, limits, after);
        let Some(first) = candidates.first() else {
            return Ok(());
        };
        // Most steps end here, before any query: even all of it would not be enough.
        let most: u64 = candidates.iter().map(|c| c.result.content_tokens).sum();
        if matches!(when, When::ItPays(_)) && most < MIN_SAVED_TOKENS {
            return Ok(());
        }
        let ends = self
            .state
            .call_ends(&self.thread_id, first.result.sequence)
            .await?;
        let Some(planned) = plan(conversation, &candidates, &ends) else {
            return Ok(());
        };
        if let When::ItPays(config) = when
            && !self.pays(&planned, config).await?
        {
            return Ok(());
        }
        self.write(conversation, planned).await
    }

    /// The warm rule unless the provider is of the Anthropic family, then the cold rule.
    async fn pays(&self, planned: &Move, config: &Config) -> Result<bool> {
        if planned.saved < MIN_SAVED_TOKENS {
            return Ok(false);
        }
        if warm_pays(planned) && !self.anthropic(config) {
            return Ok(true);
        }
        let last = self.state.last_model_call_at(&self.thread_id).await?;
        Ok(last.is_some_and(|at| now_ms().saturating_sub(at) > COLD_AFTER_MS))
    }

    /// Anthropic requests cache through one automatic breakpoint, which is thought to look
    /// back only about 20 blocks (unverified), so a move on a warm cache there would
    /// likely lose the whole prompt. Until an explicit breakpoint marks the newest stub,
    /// those providers move only when the cache is cold.
    fn anthropic(&self, config: &Config) -> bool {
        self.model.current().is_some_and(|choice| {
            let providers = config.all_providers();
            let kind = providers.get(&choice.provider_name).map(|p| p.kind);
            kind == Some(ProviderKind::Anthropic)
        })
    }

    /// Stores the stubs in one transaction, made at the newest message so the token count
    /// knows which provider counts held the whole results, then loads them back so this
    /// request sends exactly what a later load sends.
    async fn write(&self, conversation: &mut Conversation, planned: Move) -> Result<()> {
        let made_at = conversation.newest_sequence();
        let state = &self.state;
        state
            .add_stubs(&self.thread_id, made_at, &planned.stubs)
            .await?;
        conversation.stubs = state
            .stubs(&self.thread_id, conversation.covered_through())
            .await?;
        self.emit(ThreadEvent::ResultsStubbed {
            results: u32::try_from(planned.stubs.len()).unwrap_or(u32::MAX),
            saved_tokens: planned.saved,
        });
        Ok(())
    }
}

/// The newest message that has a stub: a move changes only messages after it, so the
/// prefix earlier requests sent stays as the provider cached it.
fn frontier(conversation: &Conversation) -> u64 {
    conversation
        .messages
        .iter()
        .filter(|message| conversation.stubs.contains_key(&message.message_id))
        .map(|message| message.sequence)
        .max()
        .unwrap_or(0)
}

/// The results after `after`, in order, that may be put away: of a tool on the list, at
/// least `stub_min_tokens` large as sent (shaped, when it was), with no stub yet, and
/// older than the newest `stub_keep_steps` steps (2 for a large one). A step is an
/// assistant message with tool calls; a result's age is that of the step that called it.
fn candidates<'a>(
    conversation: &'a Conversation,
    limits: &Limits,
    after: u64,
) -> Vec<Candidate<'a>> {
    let messages = &conversation.messages;
    let steps: Vec<u64> = messages
        .iter()
        .filter(|message| message.role == MessageRole::Assistant)
        .filter(|message| !message.tool_calls.is_empty())
        .map(|message| message.sequence)
        .collect();
    let calls = calls_of(messages);
    let keep = usize::try_from(limits.stub_keep_steps).unwrap_or(usize::MAX);
    messages
        .iter()
        .filter(|message| message.role == MessageRole::Tool && message.sequence > after)
        .filter(|message| conversation.sent_tokens(message) >= limits.stub_min_tokens)
        .filter(|message| !conversation.stubs.contains_key(&message.message_id))
        .filter_map(|result| {
            let (step, call) = calls.get(result.message_id.as_str())?;
            let tool = *STUBBED_TOOLS.iter().find(|tool| **tool == call.name)?;
            // The steps from the one that made the call on, itself included.
            let newer = steps.len() - steps.partition_point(|&at| at < step.sequence);
            let kept = match conversation.sent_tokens(result) >= BIG_RESULT_TOKENS {
                true => BIG_KEEP_STEPS.min(keep),
                false => keep,
            };
            let candidate = Candidate {
                result,
                tool,
                arguments: &call.arguments,
            };
            (newer > kept).then_some(candidate)
        })
        .collect()
}

/// The stubs for `candidates` and what they free; none for a stub that would not be
/// shorter than its result as sent. `None` when nothing is left.
fn plan(
    conversation: &Conversation,
    candidates: &[Candidate],
    ends: &HashMap<String, CallEnd>,
) -> Option<Move> {
    let mut stubs = Vec::new();
    let mut saved = 0;
    let mut first = None;
    for candidate in candidates {
        let result = candidate.result;
        let text = stub_text(&Stored {
            sequence: result.sequence,
            tool: candidate.tool,
            arguments: candidate.arguments,
            end: ends.get(&result.message_id).copied(),
            content: &result.content,
            tokens: result.content_tokens,
        });
        let freed = saved_tokens(conversation.sent_tokens(result), &text);
        if freed == 0 {
            continue;
        }
        saved += freed;
        first.get_or_insert(result.sequence);
        let message_id = result.message_id.clone();
        stubs.push(NewStub { message_id, text });
    }
    let first = first?;
    // System messages are sent first whatever their place, so they are not after a stub.
    let after: u64 = conversation
        .messages
        .iter()
        .filter(|message| message.sequence >= first && message.role != MessageRole::System)
        .map(|message| conversation.sent_tokens(message))
        .sum();
    Some(Move {
        stubs,
        saved,
        remaining: after.saturating_sub(saved),
    })
}

/// The warm rule: S >= 0.6 R. The floor on S is checked apart.
fn warm_pays(planned: &Move) -> bool {
    let (share, whole) = WARM_SHARE;
    planned.saved.saturating_mul(whole) >= planned.remaining.saturating_mul(share)
}
