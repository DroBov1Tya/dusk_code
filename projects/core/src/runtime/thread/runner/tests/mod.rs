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
        ChatMessage, ChatRequest, ChatStream, FinishReason, Model, ModelProvider, Transient,
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
/// The messages of every request are kept.
struct FakeProvider {
    scripts: Mutex<VecDeque<Vec<Step>>>,
    requests: Mutex<Vec<Vec<ChatMessage>>>,
    efforts: Mutex<Vec<Option<crate::config::Effort>>>,
}

#[derive(Clone)]
enum Step {
    Text(&'static str),
    /// Calls the `echo` tool with this text.
    Call(&'static str),
    Tool(String, Value),
    Hang,
    Done,
    /// The call fails before any output, in a way worth retrying.
    Fail,
    /// The connection drops where this step stands.
    Break,
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
        // Agent steps only: titles and summaries go without tools.
        if !request.tools.is_empty() {
            self.efforts.lock().unwrap().push(request.effort);
        }
        let script = self.scripts.lock().unwrap().pop_front().unwrap_or_default();
        if let Some(Step::Fail) = script.first() {
            return Box::pin(async { Err(Transient("server busy".into()).into()) });
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
            Ok(match self.steps.pop_front() {
                Some(Step::Text(text)) => Some(ModelEvent::Text(text.into())),
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
                Some(Step::Hang) => std::future::pending().await,
                Some(Step::Done) => Some(ModelEvent::Finished(FinishReason::Stop)),
                Some(Step::Break) => return Err(Transient("connection reset".into()).into()),
                Some(Step::Fail) | None => None,
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

fn provider(scripts: Vec<Vec<Step>>) -> Arc<FakeProvider> {
    Arc::new(FakeProvider {
        scripts: Mutex::new(scripts.into()),
        requests: Mutex::new(Vec::new()),
        efforts: Mutex::new(Vec::new()),
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
    let mut tools = ToolRegistry::default();
    tools.add(tool);
    tools.set_depth(0);
    let choice = Choice {
        provider_name: "fake".into(),
        provider: Arc::clone(provider) as Arc<dyn ModelProvider>,
        model,
    };
    // The size of the real prompt must not decide what fits in these small limits.
    std::fs::create_dir_all(root.join("modes")).unwrap();
    std::fs::write(root.join("modes/MAIN.md"), MAIN_PROMPT).unwrap();
    ThreadRunner::new(ThreadSetup {
        state: Arc::clone(state),
        model: Arc::new(ModelChoice::new(choice)),
        config: Arc::clone(config),
        thread_id: "thread".into(),
        tools,
        scope: Arc::new(ToolScope::new(&root, &root).unwrap()),
        project: String::new(),
        compact_prompt: root.join("COMPACT.md"),
        fork_prompt: root.join("FORK.md"),
        modes_dir: root.join("modes"),
        artifacts: ArtifactStore::new(root.join("artifacts")),
        depth: 0,
        mailboxes: Arc::default(),
        ends_when_idle: false,
        lock: ThreadLock::take(&root).unwrap(),
        hooks: Arc::default(),
    })
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

mod goals;
mod turns;
