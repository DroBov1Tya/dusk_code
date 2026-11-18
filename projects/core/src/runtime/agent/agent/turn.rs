//! A turn: starting it, running it, and what joins it from outside.

use super::wait::Waited;
use super::{Agent, Pace, StepEnd};
use crate::catalog::modes::Modes;
use crate::plugins::HookEvent;
use crate::runtime::agent::{Inbox, Keep, Stop};
use crate::runtime::{
    ThreadEvent,
    thread::{Conversation, start::NO_PROMPT},
};
use crate::state::{AgentPhase, Goal, GoalStatus, PendingMessage, Turn, TurnOutcome, TurnStatus};
use anyhow::Result;
use dusk_protocol::thread::TurnWait;
use serde_json::json;
use tracing::{info, warn};

/// Compactions a step that does not fit the model's context gets before the turn fails.
const MAX_SHRINKS: u32 = 2;

impl Agent {
    /// Starts a turn when something is queued and the context fits the input limit.
    pub async fn start_turn(&self) -> Result<Option<Turn>> {
        if self.model.current().is_none() {
            return Ok(None);
        }
        let stub_results = self.config.current().limits().stub_results;
        let conversation = Conversation::load(&self.state, &self.thread_id, stub_results).await?;
        if !conversation.has_queued() {
            return Ok(None);
        }
        self.take_up_mode(&conversation).await?;
        let config = self.config_now().await;
        let specs = self.tools.specs(&self.scope, &config);
        if self.context_too_large(&conversation, &specs, 0).is_some() {
            return Ok(None);
        }
        if self.depth == 0 {
            self.follow_mode(&conversation).await?;
        }
        let turn = self.state.start_turn(&self.thread_id).await?;
        if let Some(turn) = &turn {
            info!("thread {} turn {} starts", self.thread_id, turn.turn_id);
            *self.pace.lock() = Pace::Plan;
            self.emit(ThreadEvent::TurnStarted(turn.clone()));
            self.hook_notes(HookEvent::TurnStart, json!({})).await?;
            // A new task starts with planning; a paused one goes on.
            match self.phase().await {
                None | Some(AgentPhase::Done) => self.runtime_phase(AgentPhase::Planning).await,
                Some(AgentPhase::Paused) => self.runtime_phase(AgentPhase::Executing).await,
                Some(_) => {}
            }
        }
        Ok(turn)
    }

    /// A chat takes up the prompt of its mode when a turn starts, so a switch, or an
    /// edit of the prompt, applies from the next turn. The cached prefix changes once
    /// then, and only then.
    pub(super) async fn follow_mode(&self, conversation: &Conversation) -> Result<()> {
        let mode = self.config_now().await.mode().to_owned();
        let prompt = Modes::new(&self.modes_dir).prompt(&mode)?;
        let prompt = prompt.as_deref().unwrap_or(NO_PROMPT);
        if conversation.prompt() != Some(prompt) {
            info!("thread {} takes up the {mode} prompt", self.thread_id);
            self.state
                .add_system_message(&self.thread_id, prompt)
                .await?;
        }
        Ok(())
    }

    /// Runs the turn to its end and records it. Returns why it stopped early, if it did,
    /// and how it ended.
    pub async fn run_turn(
        &self,
        turn: &Turn,
        inbox: &mut Inbox,
    ) -> Result<(Option<Stop>, TurnStatus)> {
        self.run_turn_from(turn, inbox, None).await
    }

    /// The same, for a turn whose step already ended as `first` says.
    #[tracing::instrument(
        name = "turn",
        skip_all,
        fields(thread = %turn.thread_id, turn = %turn.turn_id, depth = self.depth)
    )]
    pub(super) async fn run_turn_from(
        &self,
        turn: &Turn,
        inbox: &mut Inbox,
        mut first: Option<StepEnd>,
    ) -> Result<(Option<Stop>, TurnStatus)> {
        // The user's stop and the context's compaction end a long run, not a step count.
        let outcome;
        let mut stop = None;
        // When the provider waits in a row began, while one is stored with the turn.
        let mut waiting_since = match turn.waiting {
            Some(TurnWait::Provider { since, .. }) => Some(since),
            _ => None,
        };
        // Compactions of a context too large for the step at hand.
        let mut shrunk = 0;
        loop {
            // A failed step ends the turn as failed; the thread itself goes on.
            let step = match first.take() {
                Some(end) => Ok(end),
                None => {
                    let waited = waiting_since.is_some();
                    self.step(turn, inbox, waited).await
                }
            };
            let end = match step {
                Ok(end) => end,
                Err(error) => {
                    outcome = TurnOutcome::failed(format!("{error:#}"));
                    break;
                }
            };
            // A step that called tools had its answer, which ended the wait it followed.
            if matches!(end, StepEnd::CalledTools) {
                waiting_since = None;
                shrunk = 0;
            }
            // The same error came back too often: another step would only spend the turn.
            // A stop by the user wins, so only a step that went on to the next is checked.
            let repeated = match end {
                StepEnd::CalledTools => self.repeats.lock().take_stop(&turn.turn_id),
                _ => None,
            };
            if let Some(why) = repeated {
                outcome = TurnOutcome::Failed(dusk_protocol::thread::TurnFailure {
                    error: why,
                    provider: None,
                });
                break;
            }
            match end {
                // Tool results are what fills the context, so it is checked after each.
                StepEnd::CalledTools => match self.compact_if_needed(Some(turn), inbox).await {
                    None => {
                        if let Err(error) = self.take_agent_messages().await {
                            outcome = TurnOutcome::failed(format!("{error:#}"));
                            break;
                        }
                        continue;
                    }
                    Some(reason) => {
                        stop = Some(reason);
                        outcome = TurnOutcome::Interrupted(None);
                    }
                },
                StepEnd::Wait(error, failure) => {
                    let waited = self.wait_for_provider(turn, &error, waiting_since, inbox);
                    match waited.await {
                        Ok(Waited::Again { since }) => {
                            waiting_since = Some(since);
                            continue;
                        }
                        Ok(Waited::Stopped(reason)) => {
                            stop = Some(reason);
                            outcome = TurnOutcome::Interrupted(None);
                        }
                        Ok(Waited::Spent) => outcome = TurnOutcome::Failed(failure),
                        Err(error) => outcome = TurnOutcome::failed(format!("{error:#}")),
                    }
                }
                StepEnd::TooLarge(_) if shrunk < MAX_SHRINKS => {
                    shrunk += 1;
                    // A tail kept the first time may be what does not fit; the second
                    // time everything goes.
                    let keep = match shrunk {
                        1 => Keep::Tail,
                        _ => Keep::Nothing,
                    };
                    info!(
                        "thread {} turn {}: the context is too large, compaction {shrunk}",
                        self.thread_id, turn.turn_id
                    );
                    match self.compact(None, keep, Some(turn), inbox).await {
                        None => continue,
                        Some(reason) => {
                            stop = Some(reason);
                            outcome = TurnOutcome::Interrupted(None);
                        }
                    }
                }
                StepEnd::TooLarge(failure) => outcome = TurnOutcome::Failed(failure),
                StepEnd::Answered(answer) => outcome = answer,
                StepEnd::Stopped(reason, partial) => {
                    stop = Some(reason);
                    outcome = partial;
                }
            }
            break;
        }
        if let TurnOutcome::Completed(answer) = &outcome {
            self.store_lesson(&turn.turn_id, &answer.content).await;
        }
        let failure = match &outcome {
            TurnOutcome::Failed(failure) => Some(failure.clone()),
            _ => None,
        };
        let turn = self.state.finish_turn(&turn.turn_id, outcome).await?;
        let took = turn.finished_at.unwrap_or(turn.started_at) - turn.started_at;
        match &turn.error_message {
            Some(error) => warn!(
                "thread {} turn {} {:?} after {took} ms: {error}",
                self.thread_id, turn.turn_id, turn.status
            ),
            None => info!(
                "thread {} turn {} {:?} after {took} ms",
                self.thread_id, turn.turn_id, turn.status
            ),
        }
        let status = turn.status;
        let ended = match status {
            TurnStatus::Completed => Some(AgentPhase::Done),
            TurnStatus::Failed | TurnStatus::Interrupted => Some(AgentPhase::Paused),
            TurnStatus::Running => None,
        };
        // An answer is the end of the task, unless the agent paused itself to wait.
        let now = self.phase().await;
        if let Some(phase) = ended
            && now.is_some_and(AgentPhase::is_open)
        {
            self.runtime_phase(phase).await;
        }
        let details = json!({ "status": format!("{status:?}").to_lowercase() });
        // A turn whose step failed ends normally for the runner; without this the clients
        // would show it as done with no word of why there is no answer. It is the one
        // event of the failure: `Failed` stays for a thread that stops.
        if let Some(failure) = failure {
            self.emit(ThreadEvent::TurnFailed(failure));
        }
        self.emit(ThreadEvent::TurnFinished(turn));
        self.report_tokens().await;
        self.hook_notes(HookEvent::TurnEnd, details).await?;
        Ok((stop, status))
    }

    /// Only a chat has a goal: the user and the chat's own model set it.
    pub async fn active_goal(&self) -> Result<Option<Goal>> {
        if self.depth > 0 {
            return Ok(None);
        }
        let goal = self.state.goal(&self.thread_id).await?;
        Ok(goal.filter(|goal| goal.status == GoalStatus::Active && goal.approved))
    }

    /// Messages from other agents join the running turn, so the next step reads them,
    /// and so does the answer to the question on the user's screen. Anything else the
    /// user sent waits for the next turn.
    pub(super) async fn take_agent_messages(&self) -> Result<()> {
        for pending in self.state.pending_messages(&self.thread_id).await? {
            if pending.from_thread_id.is_none() && pending.from_job_id.is_none() {
                if self.answers_question(&pending.content) {
                    self.take_answer(&pending).await?;
                }
                continue;
            }
            let message = self.state.admit_pending(&self.thread_id, &pending).await?;
            if message.from_thread_id.is_some() {
                sqlx::query("UPDATE goals SET work_epoch = work_epoch + 1 WHERE thread_id = ? AND approved_at IS NOT NULL")
                    .bind(&self.thread_id).execute(&self.state.pool).await?;
            }
            self.emit(ThreadEvent::Queued {
                message_id: message.message_id.clone(),
            });
            self.emit(if message.from_job_id.is_some() {
                ThreadEvent::FromJob(message)
            } else {
                ThreadEvent::FromAgent(message)
            });
        }
        Ok(())
    }

    pub(super) fn answers_question(&self, content: &str) -> bool {
        // Several questions asked in a row are answered by one message, in any order.
        self.asking
            .lock()
            .iter()
            .any(|question| content.starts_with(&format!("QUESTION: {question}\n")))
    }

    pub(super) async fn take_answer(&self, pending: &PendingMessage) -> Result<()> {
        self.asking.lock().clear();
        let message = self.state.admit_pending(&self.thread_id, pending).await?;
        sqlx::query("UPDATE goals SET awaiting_user = 0 WHERE thread_id = ?")
            .bind(&self.thread_id)
            .execute(&self.state.pool)
            .await?;
        self.emit(ThreadEvent::Queued {
            message_id: message.message_id.clone(),
        });
        self.emit(ThreadEvent::UserMessage(message));
        Ok(())
    }
}
