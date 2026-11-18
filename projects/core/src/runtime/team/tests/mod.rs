//! The team's runtime against a scripted model: a real core on a database of its own,
//! a chat that leads, and members that answer what the test's script says. No test
//! waits for the 30 s tick: a pass is run at the time the test names.

mod budgets;
mod ends;
mod limits;
mod recovery;
mod runs;
mod text;

use std::{
    collections::VecDeque,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use dusk_protocol::thread::{TeamBudget, TeamEnd, TeamStatus};
use serde_json::Value;
use tokio::sync::Semaphore;

use super::Runner;
use crate::{
    BoxFuture,
    init::paths::Paths,
    providers::{
        ChatMessage, ChatRequest, ChatStream, FinishReason, Model, ModelEvent, ModelProvider,
    },
    runtime::{Launcher, Runtime, ThreadEvent, ThreadInput, model_choice::Choice},
    state::{NewTask, PlanStatus, PlanTask, State, TeamCeilings},
    testing::temp_dir,
    tools::TeamRunner,
};

/// What a member answers when the script has nothing else for it.
pub(super) const DONE: &str =
    "Made the change.\n\nStatus: done\nResult: it works now\nChecked: not run\nOpen: nothing";

/// What the scripted model does with one call.
pub(super) enum Out {
    Say(String),
    Call(&'static str, Value),
    /// The call fails in a way not worth retrying, so the turn fails.
    Reject,
    /// The call never answers, like a member still at work.
    Hang,
    /// Answers `Say` once the test lets one member through (`Rig::let_through`).
    Held(String),
}

/// One call as the script sees it.
pub(super) struct Asked {
    pub lead: bool,
    /// The newest user message: a member's card, a line to the lead, the user's words.
    pub user: String,
    /// The newest tool result, when it is the last message of the request.
    pub result: Option<String>,
}

type Script = Box<dyn Fn(&Asked) -> Out + Send + Sync>;

pub(super) struct Scripted {
    script: Script,
    lead: Mutex<String>,
    gate: Arc<Semaphore>,
    next_call: AtomicUsize,
    /// The tools each thread was offered with its newest call.
    offered: Mutex<Vec<(String, Vec<String>)>>,
}

struct Stream {
    wait: Option<Arc<Semaphore>>,
    hang: bool,
    events: VecDeque<ModelEvent>,
}

impl ModelProvider for Scripted {
    fn list_models(&self) -> BoxFuture<'_, Vec<Model>> {
        Box::pin(async { Ok(Vec::new()) })
    }

    fn stream_chat<'a>(&'a self, request: ChatRequest<'a>) -> BoxFuture<'a, Box<dyn ChatStream>> {
        let usage = ModelEvent::Usage(crate::providers::Usage {
            prompt_tokens: 100,
            completion_tokens: 10,
            ..Default::default()
        });
        let say = |text: String| {
            VecDeque::from([
                ModelEvent::Text(text),
                usage.clone(),
                ModelEvent::Finished(FinishReason::Stop),
            ])
        };
        let plain = |events| Stream {
            wait: None,
            hang: false,
            events,
        };
        // Titles and summaries go without tools and are no part of a script.
        let Some(thread) = request.cache_key.filter(|_| !request.tools.is_empty()) else {
            let stream = plain(say("A title".into()));
            return Box::pin(async move { Ok(Box::new(stream) as Box<dyn ChatStream>) });
        };
        let names = request.tools.iter().map(|tool| tool.name.clone()).collect();
        self.offered
            .lock()
            .unwrap()
            .push((thread.to_owned(), names));
        let user = request
            .messages
            .iter()
            .rev()
            .find_map(|message| match message {
                ChatMessage::User(text) => Some(text.clone()),
                _ => None,
            });
        let result = match request.messages.last() {
            Some(ChatMessage::Tool { content, .. }) => Some(content.clone()),
            _ => None,
        };
        let asked = Asked {
            lead: *self.lead.lock().unwrap() == thread,
            user: user.unwrap_or_default(),
            result,
        };
        let stream = match (self.script)(&asked) {
            Out::Say(text) => plain(say(text)),
            Out::Held(text) => Stream {
                wait: Some(Arc::clone(&self.gate)),
                ..plain(say(text))
            },
            Out::Hang => Stream {
                hang: true,
                ..plain(VecDeque::new())
            },
            Out::Call(name, arguments) => {
                let id = self.next_call.fetch_add(1, Ordering::Relaxed);
                plain(VecDeque::from([
                    ModelEvent::ToolCall {
                        index: 0,
                        id: format!("call_{id}"),
                        name: name.into(),
                    },
                    ModelEvent::ToolArguments {
                        index: 0,
                        fragment: arguments.to_string(),
                    },
                    usage,
                    ModelEvent::Finished(FinishReason::ToolCalls),
                ]))
            }
            Out::Reject => return Box::pin(async { Err(anyhow::anyhow!("bad request")) }),
        };
        Box::pin(async move { Ok(Box::new(stream) as Box<dyn ChatStream>) })
    }
}

impl ChatStream for Stream {
    fn next_event(&mut self) -> BoxFuture<'_, Option<ModelEvent>> {
        Box::pin(async move {
            if self.hang {
                std::future::pending::<()>().await;
            }
            if let Some(gate) = self.wait.take() {
                gate.acquire().await?.forget();
            }
            Ok(self.events.pop_front())
        })
    }
}

/// A core with a chat that leads and a scripted model.
pub(super) struct Rig {
    pub runtime: Runtime,
    pub lead: String,
    pub provider: Arc<Scripted>,
    pub events: Arc<Mutex<Vec<ThreadEvent>>>,
    pub folder: PathBuf,
    /// Dropping it would end the chat's runner.
    pub input: ThreadInput,
}

/// The settings every test starts from: the plan form on, in the mode that offers it.
pub(super) const ON: &str = "mode: ORCHESTRATOR\nlimits:\n  plan_graph: true\n";

impl Rig {
    pub async fn new(name: &str, script: impl Fn(&Asked) -> Out + Send + Sync + 'static) -> Self {
        Self::with(name, ON, script).await
    }

    pub async fn with(
        name: &str,
        settings: &str,
        script: impl Fn(&Asked) -> Out + Send + Sync + 'static,
    ) -> Self {
        let root = temp_dir(name);
        let config = root.join("config");
        let folder = root.join("project");
        std::fs::create_dir_all(&config).unwrap();
        std::fs::create_dir_all(folder.join("src")).unwrap();
        let file = config.join("config.yml");
        std::fs::write(&file, settings).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let private = |mode| std::fs::Permissions::from_mode(mode);
            std::fs::set_permissions(&config, private(0o700)).unwrap();
            std::fs::set_permissions(&file, private(0o600)).unwrap();
        }
        let paths = Paths::new(config, root.join("work"));
        let runtime = Runtime::start(paths, Some(root.join("state.db")))
            .await
            .unwrap();
        let provider = Arc::new(Scripted {
            script: Box::new(script),
            lead: Mutex::default(),
            gate: Arc::new(Semaphore::new(0)),
            next_call: AtomicUsize::new(0),
            offered: Mutex::default(),
        });
        runtime.launcher.model.set(Choice {
            provider_name: "fake".into(),
            provider: Arc::clone(&provider) as Arc<dyn ModelProvider>,
            model: Model {
                id: "scripted".into(),
                context_window: None,
                max_output: None,
            },
        });
        let chat = runtime.launcher.new_thread("test", &folder).await.unwrap();
        *provider.lead.lock().unwrap() = chat.thread_id.clone();
        let events = Arc::new(Mutex::new(Vec::new()));
        let heard = Arc::clone(&events);
        let mut output = chat.output;
        tokio::spawn(async move {
            while let Some(event) = output.next_event().await {
                heard.lock().unwrap().push(event);
            }
        });
        Self {
            runtime,
            lead: chat.thread_id,
            provider,
            events,
            folder,
            input: chat.input,
        }
    }

    pub fn launcher(&self) -> &Launcher {
        &self.runtime.launcher
    }

    pub fn state(&self) -> &State {
        &self.runtime.launcher.state
    }

    /// Adds tasks to the lead's plan, as its `add` would; answers their ids.
    pub async fn plan(&self, tasks: &[NewTask]) -> Vec<String> {
        let added = self.state().add_plan_tasks(&self.lead, tasks).await;
        added
            .unwrap()
            .into_iter()
            .map(|task| task.task_id)
            .collect()
    }

    /// The lead's `run` with the budget the settings allow, or the parts given.
    pub async fn run(&self) -> usize {
        self.run_with(|budget| budget).await.unwrap()
    }

    pub async fn run_with(
        &self,
        change: impl FnOnce(TeamBudget) -> TeamBudget,
    ) -> anyhow::Result<usize> {
        let limits = self.launcher().config.current().limits();
        let budget = change(TeamCeilings::from_limits(&limits)?.budget());
        self.runner().run(&self.lead, budget).await
    }

    pub fn runner(&self) -> Runner {
        Runner::new(self.launcher().clone())
    }

    pub async fn pass(&self) -> usize {
        let now = crate::state::timestamp_ms().unwrap();
        self.launcher().team_pass_at(&self.lead, now).await.unwrap()
    }

    pub async fn task(&self, id: &str) -> PlanTask {
        self.state().plan_task(&self.lead, id).await.unwrap()
    }

    pub async fn status(&self, id: &str) -> PlanStatus {
        self.task(id).await.status
    }

    pub async fn ended(&self) -> Option<TeamEnd> {
        match self.state().team_run(&self.lead).await.unwrap()?.status {
            TeamStatus::Ended(why) => Some(why),
            TeamStatus::Running => None,
        }
    }

    /// The threads the team made under the lead, oldest first.
    pub async fn members(&self) -> Vec<String> {
        let threads = self.state().agent_threads(&self.lead).await.unwrap();
        let mut members: Vec<_> = threads.into_iter().filter(super::is_member).collect();
        members.sort_by(|a, b| (a.created_at, &a.id).cmp(&(b.created_at, &b.id)));
        members.into_iter().map(|thread| thread.id).collect()
    }

    /// What other threads wrote to the lead, read or still waiting, in order.
    pub async fn lines(&self) -> Vec<String> {
        let state = self.state();
        let read = state.list_messages(&self.lead).await.unwrap();
        let waiting = state.pending_messages(&self.lead).await.unwrap();
        read.into_iter()
            .filter(|message| message.from_thread_id.is_some())
            .map(|message| message.content)
            .chain(
                waiting
                    .into_iter()
                    .filter(|pending| pending.from_thread_id.is_some())
                    .map(|pending| pending.content),
            )
            .collect()
    }

    /// The uids of what was delivered to `thread`, read or still waiting.
    pub async fn uids(&self, thread: &str) -> Vec<String> {
        sqlx::query_scalar(
            "SELECT request_uid FROM messages WHERE thread_id = ?1 AND request_uid IS NOT NULL
             UNION ALL SELECT request_uid FROM pending_messages WHERE thread_id = ?1",
        )
        .bind(thread)
        .fetch_all(&self.state().pool)
        .await
        .unwrap()
    }

    /// Turns of `thread` that ended, and whether one runs now.
    pub async fn turns(&self, thread: &str) -> (i64, bool) {
        let rows: Vec<(String,)> = sqlx::query_as("SELECT status FROM turns WHERE thread_id = ?")
            .bind(thread)
            .fetch_all(&self.state().pool)
            .await
            .unwrap();
        let running = rows.iter().any(|(status,)| status == "running");
        (rows.len() as i64 - i64::from(running), running)
    }

    /// Lets one held member answer.
    pub fn let_through(&self) {
        self.provider.gate.add_permits(1);
    }

    /// The lead is at rest: no turn of it runs and nothing waits for it.
    pub async fn lead_rests(&self) -> bool {
        let (_, running) = self.turns(&self.lead).await;
        !running && !self.state().has_pending(&self.lead).await.unwrap()
    }

    /// The tools `thread` was offered with its newest call.
    pub fn offered(&self, thread: &str) -> Vec<String> {
        let offered = self.provider.offered.lock().unwrap();
        let newest = offered.iter().rev().find(|(asked, _)| asked == thread);
        newest.map(|(_, names)| names.clone()).unwrap_or_default()
    }
}

/// Waits until `met` holds, for 20 s at most.
pub(super) async fn until<F, Fut>(what: &str, mut met: F)
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    for _ in 0..2000 {
        if met().await {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("never happened: {what}");
}

pub(super) fn task(text: &str) -> NewTask {
    NewTask {
        description: text.into(),
        ..NewTask::default()
    }
}

/// Members that never answer, like members still at work, and a lead that notes what
/// it is told.
pub(super) fn hang(asked: &Asked) -> Out {
    match asked.lead {
        true => Out::Say("Noted.".into()),
        false => Out::Hang,
    }
}

/// A member that answers `DONE` to everything, and a lead that notes what it is told.
pub(super) fn all_done(asked: &Asked) -> Out {
    match asked.lead {
        true => Out::Say("Noted.".into()),
        false => Out::Say(DONE.into()),
    }
}
