//! How the core's events change the client state.

use dusk_protocol::{
    model::ModelEvent,
    thread::{Note, RetryRow, ThreadEvent, TurnStatus, note, retry_row, turn_spend},
};

use std::time::Instant;

use super::{Activity, App, Entry, Focus, Poll, Tone, compact_number};

impl App {
    pub fn apply(&mut self, event: ThreadEvent) {
        // The retry row is the footer's activity: one state, replaced in place. Events
        // that end it hand over to the arms below, which name the next activity.
        match retry_row(&event) {
            Some(RetryRow::Show(retry)) => {
                self.activity = Activity::Retrying(retry, Instant::now())
            }
            Some(RetryRow::Clear) if matches!(self.activity, Activity::Retrying(..)) => {
                self.activity = Activity::Thinking;
            }
            _ => {}
        }
        if let Some(spend) = turn_spend(&event) {
            self.turn_spend = spend;
        }
        match event {
            // Crash recovery may admit a pending request already in the snapshot.
            ThreadEvent::UserMessage(message)
                if self.thread_id.as_deref() == Some(message.thread_id.as_str())
                    && self.sent_ids.insert(message.message_id.clone()) =>
            {
                self.sent.push(message.content);
            }
            ThreadEvent::UserMessage(_) => {}
            ThreadEvent::ModelSelected { name, input_limit } => {
                self.model = name;
                self.provider_available = true;
                self.limit = input_limit;
            }
            ThreadEvent::LocalUnsent(message) => {
                if self.local_unsent.iter().any(|item| item.id == message.id) {
                    return;
                }
                // Another key may have arrived before this persistence event.
                if let Some(index) = self.transcript.entries.iter().rposition(
                    |entry| matches!(entry, Entry::User { text, .. } if *text == message.content),
                ) {
                    self.transcript.remove(index);
                    self.focus = Focus::Input;
                }
                self.show_local_unsent(message);
            }
            ThreadEvent::NoModel => {
                self.provider_available = false;
                self.transcript.notice(
                    "Provider unavailable. You can continue reading this chat; use /connect or /model to choose a provider.",
                    Tone::Info,
                );
            }
            ThreadEvent::TurnStarted(turn) => {
                self.turn_running = true;
                self.activity = Activity::Thinking;
                self.turn_started_at = Some(turn.started_at);
                self.last_turn_duration_ms = None;
                self.output_samples.clear();
            }
            ThreadEvent::Model(ModelEvent::Reasoning(text)) => {
                self.provider_available = true;
                self.record_output(&text);
                self.transcript.add_reasoning(&text);
                if !self.compacting {
                    self.activity = Activity::Thinking;
                }
            }
            ThreadEvent::Model(ModelEvent::Text(text)) if self.compacting => {
                self.provider_available = true;
                self.record_output(&text);
                self.transcript.add_summary(&text);
            }
            ThreadEvent::Model(ModelEvent::Text(text)) => {
                self.provider_available = true;
                self.record_output(&text);
                self.transcript.add_answer(&text);
                self.activity = Activity::Writing;
            }
            ThreadEvent::ToolStarted(call) => {
                self.activity = Activity::Tool(call.name.clone());
                self.transcript.start_tool(call);
            }
            ThreadEvent::ToolFinished { call_id, result } => {
                self.jobs_stale = true;
                self.transcript.finish_tool(&call_id, result);
                self.activity = Activity::Thinking;
            }
            ThreadEvent::TurnFinished(turn) => {
                self.turn_running = false;
                self.last_turn_duration_ms = turn
                    .finished_at
                    .map(|finished| finished.saturating_sub(turn.started_at).max(0) as u64);
                self.turn_started_at = None;
                self.output_samples.clear();
                self.transcript.stop_running_tools();
                match turn.status {
                    TurnStatus::Interrupted => self.transcript.notice("stopped", Tone::Info),
                    // `TurnFailed` came just before and said why.
                    TurnStatus::Failed => {}
                    _ => {
                        if let Some(ms) = self.last_turn_duration_ms {
                            let took = dusk_protocol::thread::duration_words(ms as i64);
                            self.transcript
                                .notice(format!("Worked for {took}"), Tone::Info);
                        }
                    }
                }
            }
            // The footer shows the retry in place; the transcript keeps no line of it.
            ThreadEvent::Retrying { .. } => {}
            ThreadEvent::TurnWaiting(_) => {
                if let Some(Note::Notice(text)) = note(&event) {
                    self.transcript.notice(text, Tone::Info);
                }
            }
            ThreadEvent::TurnFailed(failure) => self.error(failure.text()),
            ThreadEvent::TitleChanged { title } => self.title = Some(title),
            ThreadEvent::OutputLimitReached => {
                let text = "the answer reached the model's output limit".to_owned();
                self.transcript.notice(text, Tone::Info);
            }
            ThreadEvent::CompactionStarted => {
                self.compacting = true;
                self.activity = Activity::Compacting;
            }
            ThreadEvent::CompactionFinished(compaction) => {
                self.compacting = false;
                self.after_compaction();
                let text = format!(
                    "context compacted: messages up to #{} now live in the summary above",
                    compaction.through_sequence
                );
                self.transcript.notice(text, Tone::Info);
            }
            ThreadEvent::CompactionFailed { error } => {
                self.compacting = false;
                self.after_compaction();
                self.error(format!("compaction failed: {error}"));
            }
            ThreadEvent::Tokens { tokens, limit } => {
                self.tokens = tokens;
                self.limit = limit;
            }
            ThreadEvent::ContextTooLarge { tokens, limit } => self.error(format!(
                "the context is ~{} tokens, past the limit of {}: compact it with /compact",
                compact_number(tokens),
                compact_number(limit)
            )),
            ThreadEvent::Rejected { reason } => self.error(reason),
            ThreadEvent::Forked { summary } => self.forked = Some(summary),
            ThreadEvent::Undone { text } => self.undone = Some(text),
            ThreadEvent::Failed(error) => {
                self.error(format!("thread stopped: {error}"));
                self.stop_work();
            }
            ThreadEvent::SubagentStarted {
                thread_id,
                agent,
                task,
                effort,
                model,
                ..
            } => {
                let parent = self.thread_id.clone();
                self.start_subagent(thread_id, agent, task, (effort, model), parent);
            }
            ThreadEvent::Subagent { thread_id, event } => self.apply_subagent(&thread_id, *event),
            ThreadEvent::FromJob(message) => {
                self.jobs_stale = true;
                self.transcript.job_result(&message.content);
            }
            ThreadEvent::FromAgent(message) => {
                let from = message.from_thread_id.unwrap_or_default();
                self.transcript
                    .push(Entry::from_agent(from, message.content));
            }
            ThreadEvent::Tasks(tasks) => self.tasks = tasks,
            ThreadEvent::Goal {
                goal,
                note,
                started,
            } => self.goal_changed(goal, note, started),
            // A second question waits behind the first instead of taking its place.
            ThreadEvent::Question(question) if self.poll.is_some() => {
                self.polls_waiting.push_back(question)
            }
            ThreadEvent::Question(question) => self.poll = Some(Poll::new(question)),
            ThreadEvent::Approval(request) => self.ask_approval(request, None),
            ThreadEvent::ApprovalClosed { id } => self.close_approval(id),
            ThreadEvent::ArbiterDecided {
                what,
                allow,
                reason,
            } => self.arbiter_decided(what, allow, reason),
            ThreadEvent::PicturesRefused { model } => self.transcript.notice(
                format!("{model} does not take pictures; it gets their paths instead"),
                Tone::Info,
            ),
            ThreadEvent::AnswerCut { error } => self.error(format!(
                "the answer broke off ({error}); what it wrote is kept"
            )),
            ThreadEvent::Idle => {
                self.turn_running = false;
                self.activity = Activity::Idle;
                self.turn_started_at = None;
                self.output_samples.clear();
            }
            _ => {}
        }
    }
}
