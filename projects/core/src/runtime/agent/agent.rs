//! The agent: runs a turn over a thread step by step. Each step streams the model's
//! response; when it calls tools, they run and the next step sends their results back,
//! until the model answers without calling a tool.

use std::{path::PathBuf, sync::Arc, time::Duration};

use parking_lot::Mutex;

use anyhow::{Context, Result, anyhow};
use tokio::sync::mpsc;
use tracing::{info, warn};

use super::{Inbox, Stop, generation};
use crate::runtime::{
    chat::{Conversation, spec_tokens},
    system::{
        ThreadEvent,
        model_choice::{Choice, ModelChoice},
    },
};
use crate::state::artifacts::ArtifactStore;
use crate::{
    chat::{
        goal,
        sources::{self, SourceBlock},
    },
    config::live::LiveConfig,
    config::{Config, Effort},
    providers::{
        ChatMessage, ChatRequest, ModelOutput, ToolCall, ToolSpec, is_output_limit, is_transient,
        is_unsupported_effort, is_unsupported_pictures, text_calls,
    },
    state::{
        AgentPhase, CallPurpose, Goal, GoalStatus, MessageRole, ModelCall, PendingMessage, State,
        Turn, TurnAnswer, TurnOutcome, TurnStatus, estimate_tokens,
    },
    tools::{Progress, ToolRegistry, ToolScope},
};

/// Bounds a model that keeps calling tools; long tasks of small steps fit in it.
const MAX_STEPS: usize = 150;
/// A model call that fails before writing anything is tried this many times in all.
const MAX_ATTEMPTS: u32 = 3;
/// Doubles before each further attempt.
const FIRST_RETRY_DELAY: Duration = Duration::from_secs(1);

pub struct Agent {
    pub state: Arc<State>,
    pub model: Arc<ModelChoice>,
    pub config: Arc<LiveConfig>,
    pub tools: ToolRegistry,
    pub scope: ToolScope,
    /// Comes right after the prompt in every request.
    pub project: String,
    /// The model that refused pictures; the others still get them.
    pub pictures_refused: Mutex<Option<String>>,
    /// The question on the user's screen. Its answer joins the running turn instead of
    /// waiting for the next one.
    pub asking: Mutex<Option<String>>,
    pub thread_id: String,
    /// COMPACT.md: how to summarize the thread.
    pub compact_prompt: PathBuf,
    /// FORK.md: how to describe the thread for a new one to go on from.
    pub fork_prompt: PathBuf,
    /// The prompts of the chat's modes.
    pub modes_dir: PathBuf,
    pub artifacts: ArtifactStore,
    /// How many agents are above this thread; 0 for a chat.
    pub depth: usize,
    pub events: mpsc::UnboundedSender<ThreadEvent>,
}

/// How one step ended.
enum StepEnd {
    Answered(TurnOutcome),
    CalledTools,
    Stopped(Stop, TurnOutcome),
}

impl Agent {
    /// Marks turns left running by a stopped process as interrupted, and an agent that
    /// was at work then as paused.
    pub async fn recover(&self) -> Result<()> {
        self.state.interrupt_running_turns(&self.thread_id).await?;
        if self.phase().await.is_some_and(AgentPhase::is_open) {
            self.runtime_phase(AgentPhase::Paused).await;
        }
        self.state.recover_goal_verification(&self.thread_id).await
    }

    /// Starts a turn when something is queued and the context fits the input limit.
    pub async fn start_turn(&self) -> Result<Option<Turn>> {
        if self.model.current().is_none() {
            return Ok(None);
        }
        let conversation = Conversation::load(&self.state, &self.thread_id).await?;
        let specs = self.tools.specs(&self.scope, &self.config.current());
        if !conversation.has_queued() || self.context_too_large(&conversation, &specs, 0).is_some()
        {
            return Ok(None);
        }
        let turn = self.state.start_turn(&self.thread_id).await?;
        if let Some(turn) = &turn {
            info!("thread {} turn {} starts", self.thread_id, turn.turn_id);
            self.emit(ThreadEvent::TurnStarted(turn.clone()));
            // A new task starts with planning; a paused one goes on.
            match self.phase().await {
                None | Some(AgentPhase::Done) => self.runtime_phase(AgentPhase::Planning).await,
                Some(AgentPhase::Paused) => self.runtime_phase(AgentPhase::Executing).await,
                Some(_) => {}
            }
        }
        Ok(turn)
    }

    /// Runs the turn to its end and records it. Returns why it stopped early, if it did,
    /// and how it ended.
    #[tracing::instrument(
        name = "turn",
        skip_all,
        fields(thread = %turn.thread_id, turn = %turn.turn_id, depth = self.depth)
    )]
    pub async fn run_turn(
        &self,
        turn: &Turn,
        inbox: &mut Inbox,
    ) -> Result<(Option<Stop>, TurnStatus)> {
        let mut outcome = TurnOutcome::Failed(format!(
            "stopped after {MAX_STEPS} tool steps; what was done is kept, write to go on"
        ));
        let mut stop = None;
        for _ in 0..MAX_STEPS {
            // A failed step ends the turn as failed; the thread itself goes on.
            let end = match self.step(turn, inbox).await {
                Ok(end) => end,
                Err(error) => {
                    outcome = TurnOutcome::Failed(format!("{error:#}"));
                    break;
                }
            };
            match end {
                // Tool results are what fills the context, so it is checked after each.
                StepEnd::CalledTools => match self.compact_if_needed(Some(turn), inbox).await {
                    None => {
                        if let Err(error) = self.take_agent_messages().await {
                            outcome = TurnOutcome::Failed(format!("{error:#}"));
                            break;
                        }
                        continue;
                    }
                    Some(reason) => {
                        stop = Some(reason);
                        outcome = TurnOutcome::Interrupted(None);
                    }
                },
                StepEnd::Answered(answer) => outcome = answer,
                StepEnd::Stopped(reason, partial) => {
                    stop = Some(reason);
                    outcome = partial;
                }
            }
            break;
        }
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
        self.emit(ThreadEvent::TurnFinished(turn));
        self.report_tokens().await;
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
    async fn take_agent_messages(&self) -> Result<()> {
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

    fn answers_question(&self, content: &str) -> bool {
        self.asking
            .lock()
            .as_ref()
            .is_some_and(|question| content.starts_with(&format!("QUESTION: {question}\n")))
    }

    async fn take_answer(&self, pending: &PendingMessage) -> Result<()> {
        *self.asking.lock() = None;
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

    /// A subagent's phase; a chat has none.
    async fn phase(&self) -> Option<AgentPhase> {
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
    async fn runtime_phase(&self, phase: AgentPhase) {
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
    async fn effort(&self, config: &Config) -> Option<Effort> {
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
        let specs = self.tools.specs(&self.scope, &self.config.current());
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
                limit: self.input_limit(),
            }),
            Err(error) => warn!("cannot count thread tokens: {error:#}"),
        }
    }

    pub fn emit(&self, event: ThreadEvent) {
        // A client that stopped listening does not stop the thread.
        let _ = self.events.send(event);
    }

    /// Database writes happen outside `race`, so stopping never cuts one in half.
    #[tracing::instrument(name = "step", skip_all)]
    async fn step(&self, turn: &Turn, inbox: &mut Inbox) -> Result<StepEnd> {
        let conversation = Conversation::load(&self.state, &self.thread_id).await?;
        // Messages that joined between steps are answered by this turn, not a next one.
        let newest_user = conversation
            .messages
            .iter()
            .filter(|message| matches!(message.role, MessageRole::User | MessageRole::Job))
            .map(|message| message.sequence)
            .max()
            .unwrap_or(0);
        if newest_user > conversation.answered_through {
            self.state.extend_turn(&turn.turn_id, newest_user).await?;
        }
        // One snapshot per step: switches flipped meanwhile apply from the next step.
        let config = self.config.current();
        let mut tools = self.tools.specs(&self.scope, &config);
        let screen = |spec: &ToolSpec| {
            self.tools.kind_of(&spec.name) == Some(crate::tools::ToolKind::Computer)
        };
        if tools.iter().any(screen) && !self.sees_pictures().await {
            tools.retain(|spec| !screen(spec));
        }
        let standing = if self.depth == 0 {
            self.state.goal(&self.thread_id).await?
        } else {
            None
        };
        if standing
            .as_ref()
            .is_some_and(|g| !g.approved && g.status != GoalStatus::Done)
        {
            tools.retain(|tool| tool.name == "goal");
        }
        let workspace = self.scope.workspace();
        let with_pictures = self.takes_pictures();
        let sources =
            sources::block(&self.state, workspace, &self.thread_id, with_pictures).await?;
        // The provider's last count already holds the project text.
        let project = match conversation.last_prompt {
            Some(_) => 0,
            None => estimate_tokens(&self.project),
        };
        let attached = project + sources.as_ref().map_or(0, SourceBlock::tokens);
        // Tool results grow the context within a turn, so every step checks it.
        if let Some(error) = self.context_too_large(&conversation, &tools, attached) {
            return Ok(StepEnd::Answered(TurnOutcome::Failed(error)));
        }
        let mut messages = conversation.chat_messages();
        if with_pictures {
            self.attach_tool_pictures(&conversation, &mut messages);
        }
        let after_prompt = messages
            .iter()
            .position(|message| !matches!(message, ChatMessage::System(_)))
            .unwrap_or(messages.len());
        if !self.project.is_empty() {
            messages.insert(after_prompt, ChatMessage::System(self.project.clone()));
        }
        let first_time = sources.map(|block| {
            messages.push(block.message());
            block.first_time
        });
        // The goal is said last, so it stays in view however long the context grows.
        if let Some(goal) = standing.filter(|g| {
            g.status == GoalStatus::Active || (!g.approved && g.status != GoalStatus::Done)
        }) {
            let rounds = config.limits().max_goal_rounds;
            messages.push(ChatMessage::System(goal::reminder(&goal, rounds)));
        }
        if let Some(note) = missing_tools(&messages, &tools) {
            messages.push(ChatMessage::System(note));
        }
        let choice = self.model.current().context("provider is unavailable")?;
        let effort = self.effort(&config).await;
        let request = ChatRequest {
            model: &choice.model.id,
            messages: &messages,
            tools: &tools,
            max_output_tokens: choice.model.max_output,
            effort,
        };
        let mut called = self.call_model(&choice, request, &config, inbox).await;
        if matches!(&called, Ok((_, Ok(Err(error)))) if is_unsupported_pictures(error)) {
            self.refuse_pictures(&choice.model.id, &mut messages);
            let request = ChatRequest {
                model: &choice.model.id,
                messages: &messages,
                tools: &tools,
                max_output_tokens: choice.model.max_output,
                effort,
            };
            called = self.call_model(&choice, request, &config, inbox).await;
        }
        let (output, generated) = match called {
            Ok(called) => called,
            Err(stop) => return Ok(StepEnd::Stopped(stop, TurnOutcome::Interrupted(None))),
        };
        // Only the last attempt is recorded: a failed one rarely gets usage reported.
        if let Some(usage) = output.usage {
            let call = ModelCall {
                thread_id: self.thread_id.clone(),
                turn_id: Some(turn.turn_id.clone()),
                purpose: CallPurpose::Answer,
                compaction_id: conversation.compaction.as_ref().map(|c| c.compaction_id),
                model: choice.model.id.clone(),
                through_sequence: conversation.newest_sequence(),
            };
            if let Err(error) = self.state.record_usage(&call, usage).await {
                let error = format!("cannot store the model usage: {error:#}");
                return Ok(StepEnd::Answered(TurnOutcome::Failed(error)));
            }
        }
        let reasoning = recorded_reasoning(&config, &output);
        let (content, generated) = match generated {
            Ok(Ok(calls)) if calls.is_empty() => match text_calls::recover(&output.text, &tools) {
                Some((content, calls)) => {
                    info!(
                        "thread {}: {} tool calls written as text",
                        self.thread_id,
                        calls.len()
                    );
                    (content, Ok(Ok(calls)))
                }
                None => (output.text, Ok(Ok(calls))),
            },
            generated => (output.text, generated),
        };
        let answer = TurnAnswer { reasoning, content };
        let calls = match generated {
            Err(stop) => return Ok(StepEnd::Stopped(stop, interrupted(answer))),
            // The connection is closed already; what was written so far is kept.
            Ok(Err(error)) if is_output_limit(&error) => {
                self.emit(ThreadEvent::OutputLimitReached);
                return Ok(StepEnd::Answered(interrupted(answer)));
            }
            // The connection broke after the model wrote something: what it wrote stays in
            // the thread, so the next turn can go on from it.
            Ok(Err(error)) if !answer.content.trim().is_empty() => {
                let error = format!("{error:#}");
                self.emit(ThreadEvent::AnswerCut { error });
                return Ok(StepEnd::Answered(interrupted(answer)));
            }
            Ok(Err(error)) => {
                if is_transient(&error) {
                    self.emit(ThreadEvent::ProviderUnavailable);
                }
                return Ok(StepEnd::Answered(TurnOutcome::Failed(format!("{error:#}"))));
            }
            Ok(Ok(calls)) if calls.is_empty() => {
                // A turn cut short carries its new sources again next time.
                let first_time = first_time.unwrap_or_default();
                self.state.mark_sources_delivered(&first_time).await?;
                return Ok(StepEnd::Answered(TurnOutcome::Completed(answer)));
            }
            Ok(Ok(calls)) => calls,
        };

        // On record before anything runs, so no tool takes effect without a trace.
        let recorded = self
            .state
            .record_tool_calls(&turn.turn_id, answer, calls.clone());
        if let Err(error) = recorded.await {
            let error = format!("cannot store the tool calls: {error:#}");
            return Ok(StepEnd::Answered(TurnOutcome::Failed(error)));
        }
        Ok(match self.call_tools(turn, &calls, &config, inbox).await {
            Err(error) => StepEnd::Answered(TurnOutcome::Failed(format!("{error:#}"))),
            Ok(Some(stop)) => StepEnd::Stopped(stop, TurnOutcome::Interrupted(None)),
            Ok(None) => StepEnd::CalledTools,
        })
    }

    /// Streams the model's response, trying again when the call fails in a way that may
    /// pass and nothing reached the user yet. The outer `Err` means the user stopped it
    /// during a pause between attempts.
    pub async fn call_model(
        &self,
        choice: &Choice,
        request: ChatRequest<'_>,
        requested_config: &Config,
        inbox: &mut Inbox,
    ) -> Result<(ModelOutput, Result<Result<Vec<ToolCall>>, Stop>), Stop> {
        let mut attempt = 1;
        loop {
            let mut output = ModelOutput::default();
            let events = self.events.clone();
            let streamed = generation::generate(&*choice.provider, request, &mut output, |event| {
                let _ = events.send(ThreadEvent::Model(event.clone()));
            });
            let mut generated = inbox.race(streamed).await;
            if request.effort == Some(Effort::Off)
                && let Ok(Err(error)) = &mut generated
                && is_unsupported_effort(error)
            {
                let mut restored = false;
                let saved = self.config.update(|config| {
                    restored = config.restore_unsupported_effort(requested_config);
                });
                let detail = match saved {
                    Ok(config) if restored => format!(
                        "previous reasoning effort restored to {}",
                        config.effort().map_or("default", Effort::name)
                    ),
                    Ok(_) => "reasoning effort changed meanwhile; current setting kept".to_owned(),
                    Err(save_error) => {
                        format!("cannot restore previous reasoning effort: {save_error:#}")
                    }
                };
                let rejected = std::mem::replace(error, anyhow!("reasoning effort rejected"));
                *error = rejected.context(detail);
            }
            let error = match &generated {
                Ok(Err(error))
                    if attempt < MAX_ATTEMPTS && !output.answered() && is_transient(error) =>
                {
                    format!("{error:#}")
                }
                _ => return Ok((output, generated)),
            };
            warn!(
                "thread {} model call failed, retry {attempt}: {error}",
                self.thread_id
            );
            self.emit(ThreadEvent::Retrying { attempt, error });
            let pause = FIRST_RETRY_DELAY * 2u32.pow(attempt - 1);
            inbox.race(tokio::time::sleep(pause)).await?;
            attempt += 1;
        }
    }

    /// Calls run one by one, and each result is stored as soon as it is known. After a
    /// stop the rest are not run, but each still gets a result, because the model expects
    /// one per call. An error means a result could not be stored.
    async fn call_tools(
        &self,
        turn: &Turn,
        calls: &[ToolCall],
        config: &Config,
        inbox: &mut Inbox,
    ) -> Result<Option<Stop>> {
        let mut stop = None;
        for call in calls {
            let mut artifacts = Vec::new();
            let mut finished = None;
            let mut outcome = "cancelled";
            let mut exit_code = None;
            let draft = self.depth == 0
                && self
                    .state
                    .goal(&self.thread_id)
                    .await?
                    .is_some_and(|g| !g.approved && g.status != GoalStatus::Done);
            let content = match stop {
                Some(_) => "not run: the user interrupted the turn".to_owned(),
                None if draft && call.name != "goal" => {
                    outcome = "failed";
                    "error: draft goal execution is blocked until the user approves its acceptance contract".to_owned()
                }
                None => {
                    self.emit(ThreadEvent::ToolStarted(call.clone()));
                    let progress = Progress {
                        call_id: &call.id,
                        events: &self.events,
                    };
                    let called = self.tools.call(call, &self.scope, config, progress);
                    match inbox.race(called).await {
                        Ok(result) => {
                            let content = result.content.clone();
                            outcome = if result.is_error {
                                "failed"
                            } else {
                                "succeeded"
                            };
                            exit_code = result.exit_code;
                            artifacts = result.artifacts.clone();
                            finished = Some(result);
                            content
                        }
                        Err(reason) => {
                            stop = Some(reason);
                            "stopped: the user interrupted the turn".to_owned()
                        }
                    }
                }
            };
            let stored = self.state.record_tool_result_with_outcome(
                &turn.turn_id,
                &call.id,
                content,
                artifacts,
                outcome,
                exit_code,
            );
            stored.await.context("cannot store the tool result")?;
            if call.name == "ask_user"
                && let Some(result) = &finished
                && !result.is_error
                && let Some(question) = call.arguments["question"].as_str()
            {
                *self.asking.lock() = Some(question.to_owned());
            }
            // A visible result must already be available when the thread is reopened.
            if let Some(result) = finished {
                self.emit(ThreadEvent::ToolFinished {
                    call_id: call.id.clone(),
                    result,
                });
            }
        }
        Ok(stop)
    }

    /// The largest request under the selected model policy and its known window.
    pub fn input_limit(&self) -> u64 {
        let config = self.config.current();
        self.model
            .current()
            .map_or(crate::config::UNKNOWN_WINDOW, |choice| {
                choice.input_limit(&config)
            })
    }

    /// The pictures of the newest step's tool results, such as a screenshot, go with
    /// them. Older ones stay out: each would cost the context again in every request,
    /// and the model looks again when it needs to.
    fn attach_tool_pictures(&self, conversation: &Conversation, messages: &mut [ChatMessage]) {
        let Some(newest) = conversation
            .messages
            .iter()
            .rposition(|message| message.role == MessageRole::Assistant)
        else {
            return;
        };
        for message in &conversation.messages[newest + 1..] {
            let Some(call) = message.tool_call_id.as_ref() else {
                continue;
            };
            let pictures: Vec<_> = message
                .artifacts
                .iter()
                .filter(|artifact| artifact.for_model)
                .filter_map(|artifact| self.artifacts.read(artifact).ok())
                .filter_map(|bytes| sources::picture(&bytes))
                .collect();
            if pictures.is_empty() {
                continue;
            }
            // From the end: a server may reuse call IDs across steps.
            let slot = messages.iter_mut().rev().find_map(|chat| match chat {
                ChatMessage::Tool {
                    call_id, pictures, ..
                } if call_id == call => Some(pictures),
                _ => None,
            });
            if let Some(slot) = slot {
                *slot = pictures;
            }
        }
    }

    /// Whether the current model sees pictures: asked once, then remembered. A model
    /// that refused pictures does not.
    async fn sees_pictures(&self) -> bool {
        let Some(choice) = self.model.current() else {
            return false;
        };
        if !self.takes_pictures() {
            return false;
        }
        let name = choice.name();
        if let Some(sees) = self.model.vision(&name) {
            return sees;
        }
        if !self.model.may_ask_vision(&name) {
            return false;
        }
        match super::vision::sees_pictures(&choice).await {
            Some(sees) => {
                info!("model {name} sees pictures: {sees}");
                self.model.set_vision(name, sees);
                sees
            }
            None => {
                self.model.vision_failed(name);
                false
            }
        }
    }

    fn takes_pictures(&self) -> bool {
        let refused = self.pictures_refused.lock();
        let current = self.model.current();
        current.is_none_or(|choice| refused.as_ref() != Some(&choice.model.id))
    }

    /// Takes the pictures out of the request, and sends none to this model from now on.
    fn refuse_pictures(&self, model: &str, messages: &mut [ChatMessage]) {
        *self.pictures_refused.lock() = Some(model.to_owned());
        warn!("thread {} model {model} refused pictures", self.thread_id);
        self.emit(ThreadEvent::PicturesRefused {
            model: model.to_owned(),
        });
        for message in messages {
            match message {
                ChatMessage::UserWithPictures { text, .. } => {
                    let said = format!("{text}[this model refused the pictures named above]\n");
                    *message = ChatMessage::User(said);
                }
                ChatMessage::Tool {
                    content, pictures, ..
                } if !pictures.is_empty() => {
                    pictures.clear();
                    content.push_str("\n[this model refused the picture of this result]");
                }
                _ => {}
            }
        }
    }

    /// Reports a context past the input limit to the client and returns why.
    fn context_too_large(
        &self,
        conversation: &Conversation,
        specs: &[ToolSpec],
        attached: u64,
    ) -> Option<String> {
        let tokens = conversation.tokens(specs) + attached;
        let limit = Some(self.input_limit()).filter(|&limit| tokens > limit)?;
        self.emit(ThreadEvent::ContextTooLarge { tokens, limit });
        Some(format!(
            "the context is about {tokens} tokens, past the input limit of {limit}"
        ))
    }
}

/// What a stopped step keeps: the text written so far, if any.
fn interrupted(answer: TurnAnswer) -> TurnOutcome {
    let written = !answer.content.trim().is_empty();
    TurnOutcome::Interrupted(written.then_some(answer))
}

fn recorded_reasoning(config: &Config, output: &ModelOutput) -> Option<String> {
    let keep = config.thread().record.records_reasoning() && !output.reasoning.trim().is_empty();
    keep.then(|| output.reasoning.clone())
}

/// A model copies the calls it sees in the thread. After a switch to a mode with fewer
/// tools, it is told which of those it no longer has, so it does not reach for them.
fn missing_tools(messages: &[ChatMessage], tools: &[ToolSpec]) -> Option<String> {
    let mut missing: Vec<&str> = messages
        .iter()
        .filter_map(|message| match message {
            ChatMessage::Assistant { tool_calls, .. } => Some(tool_calls),
            _ => None,
        })
        .flatten()
        .map(|call| call.name.as_str())
        .filter(|name| !tools.iter().any(|tool| tool.name == *name))
        .collect();
    missing.sort_unstable();
    missing.dedup();
    if missing.is_empty() {
        return None;
    }
    Some(format!(
        "Earlier in this thread there are calls to tools you do not have now, since the \
         mode or the tool switches changed: {}. Do not call them; work with the tools you \
         have, or ask an agent that has them.",
        missing.join(", ")
    ))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn tools_called_before_a_mode_switch_are_named() {
        let call = |name: &str| ToolCall {
            id: name.into(),
            name: name.into(),
            arguments: json!({}),
        };
        let messages = [ChatMessage::Assistant {
            text: String::new(),
            tool_calls: vec![call("run_command"), call("ask_agent"), call("run_command")],
        }];
        let spec = |name: &str| ToolSpec {
            name: name.into(),
            description: String::new(),
            parameters: json!({}),
        };
        let note = missing_tools(&messages, &[spec("ask_agent")]).unwrap();
        assert!(note.contains(": run_command."), "{note}");
        assert!(missing_tools(&messages, &[spec("ask_agent"), spec("run_command")]).is_none());
    }
}
