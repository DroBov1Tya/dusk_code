use std::{collections::VecDeque, sync::Mutex};

use serde_json::{Value, json};

use super::*;
use crate::providers::{ModelEvent, ToolCall};
use crate::runtime::{ThreadEvent, ThreadLock, model_choice::ModelChoice};
use crate::state::artifacts::ArtifactStore;
use crate::state::{Goal, GoalStatus, Turn};
use crate::{
    BoxFuture,
    providers::{
        ChatMessage, ChatRequest, ChatStream, FinishReason, Model, ModelProvider, ProviderError,
        ProviderErrorKind,
    },
    runtime::model_choice::Choice,
    state::{MessageRole, TurnStatus},
    testing::{temp_dir, temp_live_config, temp_live_config_with, temp_state},
    tools::{Tool, ToolContext},
};
use crate::{
    config::live::LiveConfig,
    message::commands,
    state::State,
    tools::{ToolRegistry, ToolScope},
};
use std::{sync::Arc, time::Duration};

/// Each model call takes the next script; `Hang` never finishes, like a slow model.
/// The messages of every request are kept, and when it was sent, in Unix ms.
struct FakeProvider {
    scripts: Mutex<VecDeque<Vec<Step>>>,
    requests: Mutex<Vec<Vec<ChatMessage>>>,
    sent_at: Mutex<Vec<i64>>,
    efforts: Mutex<Vec<Option<crate::config::Effort>>>,
    output_limits: Mutex<Vec<Option<u64>>>,
}

#[derive(Clone)]
enum Step {
    Text(&'static str),
    Reasoning(&'static str),
    /// Calls the `echo` tool with this text.
    Call(&'static str),
    Tool(String, Value),
    /// Calls `echo` at this place of the step's calls, under this call id and with these
    /// arguments, for turns of many calls and steps of several.
    Echo(u32, String, Value),
    /// Calls the tool `name` under this call id with these arguments.
    Named(&'static str, String, Value),
    PartialCall(&'static str),
    Hang,
    /// Reports prompt and completion tokens and a cost in micro-dollars.
    Usage(u64, u64, u64),
    Done,
    Length,
    /// The call fails before any output, in a way worth retrying.
    Fail,
    /// The call cannot reach the provider at all.
    Unreachable,
    /// The call fails before any output, in a way not worth retrying.
    Reject,
    /// The call fails before any output as this kind, the provider asking to wait this
    /// long if it says.
    Error(ProviderErrorKind, Option<Duration>),
    /// The connection drops where this step stands.
    Break,
    /// The stream holds still here until the test lets it go on.
    Wait(Arc<tokio::sync::Notify>),
}

struct FakeStream {
    steps: VecDeque<Step>,
    pending: VecDeque<ModelEvent>,
}

/// Returns its `text` argument.
struct Echo;

impl ModelProvider for FakeProvider {
    fn list_models(&self) -> BoxFuture<'_, Vec<Model>> {
        Box::pin(async { Ok(Vec::new()) })
    }

    fn stream_chat<'a>(&'a self, request: ChatRequest<'a>) -> BoxFuture<'a, Box<dyn ChatStream>> {
        self.requests
            .lock()
            .unwrap()
            .push(request.messages.to_vec());
        let now = dusk_protocol::system::now_ms();
        self.sent_at.lock().unwrap().push(now);
        // Agent steps only: titles and summaries go without tools.
        if !request.tools.is_empty() {
            self.efforts.lock().unwrap().push(request.effort);
            self.output_limits
                .lock()
                .unwrap()
                .push(request.max_output_tokens);
        }
        let script = self.scripts.lock().unwrap().pop_front().unwrap_or_default();
        if let Some(Step::Fail) = script.first() {
            let busy = ProviderError::new(ProviderErrorKind::Overloaded, "server busy");
            return Box::pin(async move { Err(busy.into()) });
        }
        if let Some(Step::Unreachable) = script.first() {
            let lost = ProviderError::new(ProviderErrorKind::Unreachable, "could not reach host");
            return Box::pin(async move { Err(lost.into()) });
        }
        if let Some(&Step::Error(kind, retry_after)) = script.first() {
            let mut failed = ProviderError::new(kind, format!("{kind:?}"));
            failed.retry_after = retry_after;
            return Box::pin(async move { Err(failed.into()) });
        }
        if let Some(Step::Reject) = script.first() {
            return Box::pin(async { Err(anyhow::anyhow!("bad request")) });
        }
        let stream = FakeStream {
            steps: script.into(),
            pending: VecDeque::new(),
        };
        Box::pin(async move { Ok(Box::new(stream) as Box<dyn ChatStream>) })
    }
}

impl ChatStream for FakeStream {
    fn next_event(&mut self) -> BoxFuture<'_, Option<ModelEvent>> {
        Box::pin(async move {
            if let Some(event) = self.pending.pop_front() {
                return Ok(Some(event));
            }
            while let Some(Step::Wait(gate)) = self.steps.front() {
                let gate = Arc::clone(gate);
                self.steps.pop_front();
                gate.notified().await;
            }
            Ok(match self.steps.pop_front() {
                Some(Step::Text(text)) => Some(ModelEvent::Text(text.into())),
                Some(Step::Reasoning(text)) => Some(ModelEvent::Reasoning(text.into())),
                Some(Step::Call(text)) => {
                    self.pending.push_back(ModelEvent::ToolArguments {
                        index: 0,
                        fragment: json!({ "text": text }).to_string(),
                    });
                    Some(ModelEvent::ToolCall {
                        index: 0,
                        id: "call_1".into(),
                        name: "echo".into(),
                    })
                }
                Some(Step::Tool(name, arguments)) => {
                    self.pending.push_back(ModelEvent::ToolArguments {
                        index: 0,
                        fragment: arguments.to_string(),
                    });
                    Some(ModelEvent::ToolCall {
                        index: 0,
                        id: "goal_call".into(),
                        name,
                    })
                }
                Some(Step::Echo(index, id, arguments)) => {
                    self.pending.push_back(ModelEvent::ToolArguments {
                        index,
                        fragment: arguments.to_string(),
                    });
                    Some(ModelEvent::ToolCall {
                        index,
                        id,
                        name: "echo".into(),
                    })
                }
                Some(Step::Named(name, id, arguments)) => {
                    self.pending.push_back(ModelEvent::ToolArguments {
                        index: 0,
                        fragment: arguments.to_string(),
                    });
                    Some(ModelEvent::ToolCall {
                        index: 0,
                        id,
                        name: name.into(),
                    })
                }
                Some(Step::PartialCall(arguments)) => {
                    self.pending.push_back(ModelEvent::ToolArguments {
                        index: 0,
                        fragment: arguments.into(),
                    });
                    Some(ModelEvent::ToolCall {
                        index: 0,
                        id: "cut_call".into(),
                        name: "echo".into(),
                    })
                }
                Some(Step::Hang) => std::future::pending().await,
                Some(Step::Usage(prompt_tokens, completion_tokens, micro_usd)) => {
                    Some(ModelEvent::Usage(crate::providers::Usage {
                        prompt_tokens,
                        completion_tokens,
                        cost: Some(crate::providers::cost::Cost {
                            micro_usd,
                            source: crate::providers::cost::CostSource::Provider,
                        }),
                        ..Default::default()
                    }))
                }
                Some(Step::Done) => Some(ModelEvent::Finished(FinishReason::Stop)),
                Some(Step::Length) => Some(ModelEvent::Finished(FinishReason::Length)),
                Some(Step::Break) => {
                    let kind = ProviderErrorKind::Interrupted;
                    return Err(ProviderError::new(kind, "connection reset").into());
                }
                Some(
                    Step::Fail | Step::Unreachable | Step::Reject | Step::Error(..) | Step::Wait(_),
                )
                | None => None,
            })
        })
    }
}

impl Tool for Echo {
    fn name(&self) -> &'static str {
        "echo"
    }

    fn description(&self, _context: &ToolContext) -> String {
        "Returns its text.".into()
    }

    fn parameters(&self) -> Value {
        json!({ "type": "object" })
    }

    fn call<'a>(&'a self, arguments: Value, _context: ToolContext) -> BoxFuture<'a, String> {
        Box::pin(async move { Ok(arguments["text"].as_str().unwrap_or_default().to_owned()) })
    }
}

/// Counts its runs by their `text`, so a restart shows which calls ran again. `repeatable`
/// is what it says of a lost result, unless the call's own `repeatable` argument says
/// otherwise, and `read_only` likewise; `hang` keeps a run going once counted, as a call
/// a kill cut off after its effect. `log` has `start:text` and `end:text` in the order
/// they happened, and a call sleeps its `sleep_ms` between them, so overlap shows.
#[derive(Clone, Default)]
struct Counter {
    runs: Arc<Mutex<Vec<String>>>,
    log: Arc<Mutex<Vec<String>>>,
    repeatable: bool,
    read_only: bool,
    hang: bool,
}

impl Counter {
    fn runs(&self) -> Vec<String> {
        self.runs.lock().unwrap().clone()
    }

    fn log(&self) -> Vec<String> {
        self.log.lock().unwrap().clone()
    }
}

impl Tool for Counter {
    fn name(&self) -> &'static str {
        "echo"
    }

    fn description(&self, _context: &ToolContext) -> String {
        "Counts its runs.".into()
    }

    fn parameters(&self) -> Value {
        json!({ "type": "object" })
    }

    fn repeatable(&self, arguments: &Value) -> bool {
        arguments["repeatable"].as_bool().unwrap_or(self.repeatable)
    }

    fn read_only(&self, arguments: &Value) -> bool {
        arguments["read_only"].as_bool().unwrap_or(self.read_only)
    }

    fn call<'a>(&'a self, arguments: Value, _context: ToolContext) -> BoxFuture<'a, String> {
        Box::pin(async move {
            let text = arguments["text"].as_str().unwrap_or_default().to_owned();
            self.runs.lock().unwrap().push(text.clone());
            self.log.lock().unwrap().push(format!("start:{text}"));
            if self.hang {
                std::future::pending::<()>().await;
            }
            if let Some(ms) = arguments["sleep_ms"].as_u64() {
                tokio::time::sleep(Duration::from_millis(ms)).await;
            }
            self.log.lock().unwrap().push(format!("end:{text}"));
            Ok(text)
        })
    }
}

fn provider(scripts: Vec<Vec<Step>>) -> Arc<FakeProvider> {
    Arc::new(FakeProvider {
        scripts: Mutex::new(scripts.into()),
        requests: Mutex::new(Vec::new()),
        sent_at: Mutex::new(Vec::new()),
        efforts: Mutex::new(Vec::new()),
        output_limits: Mutex::new(Vec::new()),
    })
}

fn model(context_window: Option<u64>) -> Model {
    Model {
        id: "fake".into(),
        context_window,
        max_output: None,
    }
}

const MAIN_PROMPT: &str = "You are the main agent of duskcode, working with the user in a chat.\n\
     Answer directly and concisely, in the language the user writes in.\n";

/// A chat as `start_thread` makes it: its first message is the mode's prompt.
async fn thread_state(name: &str) -> Arc<State> {
    let state = Arc::new(temp_state(name).await);
    state.create_thread("thread", "test").await.unwrap();
    state
        .add_system_message("thread", MAIN_PROMPT)
        .await
        .unwrap();
    state
}

fn live_config(name: &str) -> Arc<LiveConfig> {
    Arc::new(temp_live_config(name))
}

fn runner(
    state: &Arc<State>,
    provider: &Arc<FakeProvider>,
    config: &Arc<LiveConfig>,
    model: Model,
) -> (ThreadRunner, ThreadInput, ThreadOutput) {
    runner_with(state, provider, config, model, Box::new(Echo))
}

/// A runner whose only tool is `tool`, which the scripts call as `echo`.
fn runner_with(
    state: &Arc<State>,
    provider: &Arc<FakeProvider>,
    config: &Arc<LiveConfig>,
    model: Model,
    tool: Box<dyn Tool>,
) -> (ThreadRunner, ThreadInput, ThreadOutput) {
    let root = temp_dir("runner_scope");
    std::fs::create_dir_all(&root).unwrap();
    let artifacts = ArtifactStore::new(root.join("artifacts"));
    runner_seeing(
        state,
        provider,
        config,
        model,
        vec![tool],
        root,
        artifacts,
        false,
    )
}

/// `runner_with` with several tools.
fn runner_with_tools(
    state: &Arc<State>,
    provider: &Arc<FakeProvider>,
    config: &Arc<LiveConfig>,
    model: Model,
    tools: Vec<Box<dyn Tool>>,
) -> (ThreadRunner, ThreadInput, ThreadOutput) {
    let root = temp_dir("runner_scope");
    std::fs::create_dir_all(&root).unwrap();
    let artifacts = ArtifactStore::new(root.join("artifacts"));
    runner_seeing(
        state, provider, config, model, tools, root, artifacts, false,
    )
}

/// `runner_with` in `root`, keeping pictures in `artifacts`; with `vision`, the model is
/// known to see pictures, so tool results carry theirs.
#[allow(clippy::too_many_arguments)]
fn runner_seeing(
    state: &Arc<State>,
    provider: &Arc<FakeProvider>,
    config: &Arc<LiveConfig>,
    model: Model,
    with: Vec<Box<dyn Tool>>,
    root: std::path::PathBuf,
    artifacts: ArtifactStore,
    vision: bool,
) -> (ThreadRunner, ThreadInput, ThreadOutput) {
    let mut tools = ToolRegistry::default();
    for tool in with {
        tools.add(tool);
    }
    tools.set_depth(0);
    let choice = Choice {
        provider_name: "fake".into(),
        provider: Arc::clone(provider) as Arc<dyn ModelProvider>,
        model,
    };
    // The size of the real prompt must not decide what fits in these small limits.
    std::fs::create_dir_all(root.join("modes")).unwrap();
    std::fs::write(root.join("modes/MAIN.md"), MAIN_PROMPT).unwrap();
    let name = choice.name();
    let model = ModelChoice::new(choice);
    if vision {
        model.set_vision(name, true);
    }
    ThreadRunner::new(ThreadSetup {
        state: Arc::clone(state),
        model: Arc::new(model),
        config: Arc::clone(config),
        thread_id: "thread".into(),
        tools,
        scope: Arc::new(ToolScope::new(&root, &root).unwrap()),
        project: String::new(),
        compact_prompt: root.join("COMPACT.md"),
        fork_prompt: root.join("FORK.md"),
        modes_dir: root.join("modes"),
        artifacts,
        depth: 0,
        mailboxes: Arc::default(),
        ends_when_idle: false,
        lock: ThreadLock::take(&root).unwrap(),
        hooks: Arc::default(),
        health: Arc::default(),
        skill_watch: Arc::new(crate::catalog::history::Watch::new(Some(
            root.join("skills"),
        ))),
        skill_state: Arc::clone(state),
    })
}

/// Every event until the runner ends.
async fn drain(output: &mut ThreadOutput) -> Vec<ThreadEvent> {
    let mut events = Vec::new();
    while let Some(event) = output.next_event().await {
        events.push(event);
    }
    events
}

async fn next_turn_finished(output: &mut ThreadOutput) -> Turn {
    loop {
        if let Some(ThreadEvent::TurnFinished(turn)) = output.next_event().await {
            return turn;
        }
    }
}

/// The chat after its opening mode prompt.
async fn transcript(state: &State) -> Vec<(MessageRole, String)> {
    let messages = state.list_messages("thread").await.unwrap();
    messages
        .into_iter()
        .skip(1)
        .map(|message| (message.role, message.content))
        .collect()
}

mod auto_compact;
mod chant;
mod chat_model;
mod episodes;
mod goals;
mod judge;
mod progress;
mod repeats;
mod resume;
mod retries;
mod shaped;
mod shrink;
mod simulator;
mod skills;
mod stubs;
mod tail;
mod together;
mod turns;
mod waits;
mod written_calls;
