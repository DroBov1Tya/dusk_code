//! What every thread is built from: the database, the model, the config and the paths.
//! The runtime builds chat threads with it, and agents build the threads of the agents
//! they ask. The messages it carries between agents are in `runtime::agent::mail`.

use crate::runtime::jobs::{JobManager, JobOwner};
use std::{path::Path, sync::Arc};

use anyhow::{Context, Result, ensure};
use tracing::{info, warn};

use crate::init::create_private_dir;
use crate::platform::Platform;
use crate::runtime::ThreadHandle;
use crate::runtime::agent::Mailboxes;
use crate::runtime::thread::project;
use crate::runtime::thread::start::{self, project_dir, start_thread};
use crate::runtime::{ThreadLock, ThreadRunner, ThreadSetup, model_choice::ModelChoice};
use crate::{
    catalog::agents::{Agents, MAX_AGENT_DEPTH},
    config::live::LiveConfig,
    init::paths::Paths,
    mcp::McpServers,
    state::artifacts::ArtifactStore,
    state::{Identity, State},
    tools::{
        AgentState, Approvals, AskAgent, AskUser, DisplayAudio, DisplayImage, GoalTool, Jobs,
        McpTool, MessageParent, PlanEdit, ReadHistory, Remember, RunCommand, Show, Tasks,
        ToolRegistry, ToolScope,
    },
};

/// How long the first chat waits for MCP servers still starting.
const MCP_START_WAIT: std::time::Duration = std::time::Duration::from_secs(10);

#[derive(Clone)]
pub struct Launcher {
    pub state: Arc<State>,
    pub model: Arc<ModelChoice>,
    pub config: Arc<LiveConfig>,
    pub paths: Paths,
    pub mailboxes: Arc<Mailboxes>,
    pub mcp: Arc<McpServers>,
    pub agents: Arc<Agents>,
    pub(crate) jobs: Arc<JobManager>,
    /// One for every thread, so a yes for the session holds in all of them.
    pub(crate) approvals: Arc<Approvals>,
    /// Found once at start; built-in tools that need what is missing stay off.
    pub platform: Platform,
    /// The tasks of open chats, which shutdown lets finish.
    pub threads: Arc<parking_lot::Mutex<tokio::task::JoinSet<()>>>,
    /// What plugins run when something happens in a thread.
    pub hooks: Arc<crate::plugins::Hooks>,
    /// How each provider's calls go, learned by every thread's calls.
    pub health: Arc<crate::providers::Health>,
    /// Its chats leave nothing behind: the database is in memory, their files are in the
    /// system's temporary folder, and they write no memory.
    pub temporary: bool,
}

impl Launcher {
    /// Where a thread's tools act: its project and workspace, the skills and agents
    /// folders, never the rest of the config folder.
    pub fn scope(&self, cwd: &Path, workspace: &Path) -> Result<ToolScope> {
        Ok(ToolScope::new(cwd, workspace)?
            .protect(self.paths.config_root())
            .share(&self.paths.skills_dir())
            .with_skills(&self.paths.skills_dir())
            .share(&self.paths.agents_dir()))
    }

    /// `agent` is the agent the thread works as, `None` for a chat or the general agent;
    /// `depth` counts the agents above it.
    pub fn setup(
        &self,
        thread_id: &str,
        cwd: &Path,
        workspace: &Path,
        agent: Option<&Identity>,
        depth: usize,
    ) -> Result<ThreadSetup> {
        Ok(ThreadSetup {
            state: Arc::clone(&self.state),
            model: Arc::clone(&self.model),
            config: Arc::clone(&self.config),
            thread_id: thread_id.to_owned(),
            tools: self.tools(thread_id, agent, depth),
            scope: Arc::new(self.scope(cwd, workspace)?),
            project: project::context(cwd),
            compact_prompt: self.paths.compact_prompt(),
            fork_prompt: self.paths.fork_prompt(),
            modes_dir: self.paths.modes_dir(),
            artifacts: ArtifactStore::new(self.paths.artifacts_dir()),
            depth,
            mailboxes: Arc::clone(&self.mailboxes),
            ends_when_idle: false,
            lock: ThreadLock::take(workspace)?,
            hooks: Arc::clone(&self.hooks),
            health: Arc::clone(&self.health),
        })
    }

    /// The built-in tools, the tools of MCP servers that are ready, the thread's own
    /// history, other agents while the depth allows, and for an agent a way to write to
    /// the thread that asked it. An agent
    /// with a `tools` list keeps only those, and that way back.
    pub fn tools(&self, thread_id: &str, agent: Option<&Identity>, depth: usize) -> ToolRegistry {
        let mut tools = ToolRegistry::builtin(&self.paths.skills_dir());
        tools.chat = depth == 0;
        tools.set_depth(depth);
        tools.jobs = Some(Arc::clone(&self.jobs));
        tools.approvals = Some(Arc::clone(&self.approvals));
        tools.platform = self.platform;
        tools.state = Some(Arc::clone(&self.state));
        tools.thread_id = thread_id.to_owned();
        for server in self.mcp.ready() {
            let pictures = ArtifactStore::new(self.paths.artifacts_dir());
            for tool in McpTool::all(&server, &pictures) {
                tools.add(Box::new(tool));
            }
        }
        let owner = JobOwner {
            manager: Arc::clone(&self.jobs),
            thread_id: thread_id.to_owned(),
        };
        tools.add(Box::new(RunCommand::with_jobs(owner.clone())));
        tools.add(Box::new(Jobs(owner)));
        let history = ReadHistory::new(Arc::clone(&self.state), thread_id.to_owned());
        tools.add(Box::new(history));
        tools.add(Box::new(Tasks::new(
            Arc::clone(&self.state),
            thread_id.to_owned(),
        )));
        // A temporary chat reads what is remembered and adds nothing to it.
        if !self.temporary {
            tools.add(Box::new(Remember::user(self.paths.rules_root())));
            tools.add(Box::new(Remember::project(
                self.paths.rules_root(),
                self.paths.workspace_root(),
            )));
        }
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        {
            use crate::tools::{
                Click, ComputerActions, PressKeys, Screen, Screenshot, Scroll, TypeText,
            };
            let screen = Arc::new(Screen::new(ArtifactStore::new(self.paths.artifacts_dir())));
            tools.add(Box::new(Screenshot(Arc::clone(&screen))));
            tools.add(Box::new(Click(Arc::clone(&screen))));
            tools.add(Box::new(Scroll(Arc::clone(&screen))));
            tools.add(Box::new(TypeText(Arc::clone(&screen))));
            tools.add(Box::new(PressKeys(Arc::clone(&screen))));
            tools.add(Box::new(ComputerActions(Arc::clone(&screen))));
            #[cfg(target_os = "macos")]
            tools.add(Box::new(crate::tools::WindowSession(screen)));
        }
        if depth < MAX_AGENT_DEPTH {
            let asker = agent.map(|agent| agent.name.clone());
            let ask = AskAgent::new(self.clone(), thread_id.to_owned(), asker);
            tools.add(Box::new(ask));
        }
        if let Some(allowed) = agent.and_then(|agent| agent.tools.as_ref()) {
            tools.keep(allowed);
        }
        // Only a chat has the user to ask.
        if depth == 0 {
            tools.add(Box::new(AskUser));
            tools.add(Box::new(Show));
            tools.add(Box::new(GoalTool::new(
                Arc::clone(&self.state),
                thread_id.to_owned(),
            )));
            tools.add(Box::new(PlanEdit::new(
                Arc::clone(&self.state),
                thread_id.to_owned(),
            )));
            tools.add(Box::new(DisplayImage::new(ArtifactStore::new(
                self.paths.artifacts_dir(),
            ))));
            tools.add(Box::new(DisplayAudio::new(ArtifactStore::new(
                self.paths.artifacts_dir(),
            ))));
        }
        if depth > 0 {
            tools.add(Box::new(MessageParent::new(
                self.clone(),
                thread_id.to_owned(),
            )));
            tools.add(Box::new(AgentState::new(
                Arc::clone(&self.state),
                thread_id.to_owned(),
            )));
        }
        tools
    }

    /// Starts a thread whose tools work in `cwd`, and the task that runs it. The task
    /// ends once the client drops every `ThreadInput` and the queue is done.
    pub async fn new_thread(&self, source: &str, cwd: &Path) -> Result<ThreadHandle> {
        self.new_thread_on(source, cwd, None).await
    }

    /// A new thread that asks `model` instead of the chosen one, as a scheduled task may.
    pub async fn new_thread_on(
        &self,
        source: &str,
        cwd: &Path,
        model: Option<Arc<ModelChoice>>,
    ) -> Result<ThreadHandle> {
        let mode = self.config.current().mode().to_owned();
        let thread = start_thread(&self.state, &self.paths, source, &mode, cwd).await?;
        info!(
            "thread {} started in {}",
            thread.thread_id,
            thread.workspace.display()
        );
        self.run_thread_on(thread.thread_id, &thread.workspace, cwd, model)
            .await
    }

    /// Picks a stored thread up again, with its tools working in `cwd`. What it left
    /// unfinished is settled first: a running turn goes on or waits for the user (see
    /// `Agent::recover`), and messages sent before a crash are taken in.
    pub async fn resume_thread(&self, thread_id: &str, cwd: &Path) -> Result<ThreadHandle> {
        let thread = self
            .state
            .load_thread(thread_id)
            .await?
            .with_context(|| format!("thread {thread_id} does not exist"))?;
        // An agent's thread runs as that agent, started by the core when it is written to.
        ensure!(
            thread.parent_thread_id.is_none(),
            "thread {thread_id} belongs to an agent"
        );
        let workspace = start::workspace(&self.state, &self.paths, thread_id).await?;
        create_private_dir(&workspace)?;
        self.state
            .adopt_thread(thread_id, &project_dir(cwd))
            .await?;
        info!("thread {thread_id} resumed in {}", workspace.display());
        let handle = self
            .run_thread(thread_id.to_owned(), &workspace, cwd)
            .await?;
        // Its agents a stopped process left at work go on, or tell it they stopped.
        if let Err(error) = self.wake_unfinished(Some(thread_id), cwd).await {
            warn!("thread {thread_id}: cannot start its agents left at work: {error:#}");
        }
        Ok(handle)
    }

    /// One task per thread: two would answer the same messages twice.
    pub async fn run_thread(
        &self,
        thread_id: String,
        workspace: &Path,
        cwd: &Path,
    ) -> Result<ThreadHandle> {
        self.run_thread_on(thread_id, workspace, cwd, None).await
    }

    async fn run_thread_on(
        &self,
        thread_id: String,
        workspace: &Path,
        cwd: &Path,
        model: Option<Arc<ModelChoice>>,
    ) -> Result<ThreadHandle> {
        // A chat opened right after the start still gets the tools of MCP servers.
        self.mcp.wait(MCP_START_WAIT).await;
        let mailboxes = Arc::clone(&self.mailboxes);
        let mut live = mailboxes.lock().await;
        ensure!(
            !live.contains_key(&thread_id),
            "thread {thread_id} is already open"
        );
        let mut setup = self.setup(&thread_id, cwd, workspace, None, 0)?;
        if let Some(model) = model {
            setup.model = model;
        }
        let (runner, input, output) = ThreadRunner::new(setup);
        live.insert(thread_id.clone(), runner.mailbox(&input));
        drop(live);
        let id = thread_id.clone();
        self.threads.lock().spawn(async move {
            runner.run().await;
            mailboxes.remove(&id).await;
        });
        Ok(ThreadHandle {
            thread_id,
            input,
            output,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use serde_json::{Value, json};

    use crate::{init::paths::Paths, runtime::Runtime};

    /// Every built-in tool, whether a call whose result was lost may run again, and
    /// whether the call only reads, so it may run with the reads next to it. A tool added
    /// without a decision here fails the test.
    const DECISIONS: &[(&str, bool, bool)] = &[
        ("agent_state", true, false),
        ("ask_agent", false, false),
        ("ask_user", true, false),
        ("click", false, false),
        ("computer_actions", false, false),
        ("display_audio", true, false),
        ("display_image", true, false),
        ("goal", false, false), // true for the show action only; see `argument_dependent_tools`
        ("jobs", false, false), // true for list, status and output only
        ("load_skill", true, true),
        ("message_parent", false, false),
        ("plan_edit", true, false),
        ("press_keys", false, false),
        ("read_file", true, true),
        ("read_history", true, true),
        ("remember", true, false),
        ("remember_project", true, false),
        ("run_command", false, false),
        ("screenshot", true, false),
        ("scroll", false, false),
        ("search_files", true, true),
        ("show", true, false),
        ("tasks", false, false),
        ("type_text", false, false),
        ("window_session", false, false),
    ];

    /// The registries a chat (depth 0) and an agent (depth 1) get, as the launcher builds
    /// them; the first call is `repeatable` with the arguments each test needs.
    async fn registries(name: &str) -> (crate::tools::ToolRegistry, crate::tools::ToolRegistry) {
        let root =
            std::env::temp_dir().join(format!("dusk-repeatable-{name}-{}", std::process::id()));
        let paths = Paths::new(root.join("config"), root.join("work"));
        let runtime = Runtime::start(paths, Some(root.join("state.db")))
            .await
            .unwrap();
        let launcher = &runtime.launcher;
        (launcher.tools("t", None, 0), launcher.tools("t", None, 1))
    }

    #[tokio::test]
    async fn every_built_in_tool_has_a_repeat_decision() {
        let (chat, agent) = registries("table").await;
        let mut built: BTreeSet<String> = BTreeSet::new();
        built.extend(chat.names().map(str::to_owned));
        built.extend(agent.names().map(str::to_owned));
        let decided: BTreeSet<String> = DECISIONS.iter().map(|(n, _, _)| (*n).to_owned()).collect();
        let undecided: Vec<_> = built.difference(&decided).collect();
        assert!(undecided.is_empty(), "no repeat decision for {undecided:?}");
        // window_session exists on macOS only.
        let missing: Vec<_> = decided
            .difference(&built)
            .filter(|name| cfg!(target_os = "macos") || *name != "window_session")
            .collect();
        assert!(
            missing.is_empty(),
            "the table names unknown tools {missing:?}"
        );
        for (name, repeatable, read_only) in DECISIONS {
            // A group of reads is run again after a restart, so a read-only call must be
            // one that may run again.
            assert!(
                *repeatable || !read_only,
                "{name} reads only, yet is not repeatable"
            );
            for registry in [&chat, &agent] {
                if registry.names().any(|n| n == *name) {
                    let got = registry.repeatable(name, &json!({}));
                    assert_eq!(got, *repeatable, "{name}");
                    let got = registry.read_only(name, &json!({}));
                    assert_eq!(got, *read_only, "{name} read-only");
                }
            }
        }
    }

    #[tokio::test]
    async fn argument_dependent_tools_repeat_only_for_reads() {
        let (chat, _) = registries("arguments").await;
        let cases: [(&str, Value, bool); 8] = [
            ("goal", json!({"action": "show"}), true),
            ("goal", json!({"action": "propose"}), false),
            ("goal", json!({"action": "request_verification"}), false),
            ("jobs", json!({"action": "list"}), true),
            ("jobs", json!({"action": "status", "job_id": "j"}), true),
            ("jobs", json!({"action": "output", "job_id": "j"}), true),
            ("jobs", json!({"action": "cancel", "job_id": "j"}), false),
            ("jobs", json!({}), false),
        ];
        for (name, arguments, expected) in cases {
            assert_eq!(
                chat.repeatable(name, &arguments),
                expected,
                "{name} {arguments}"
            );
        }
        assert!(!chat.repeatable("no_such_tool", &json!({})));
        assert!(!chat.read_only("no_such_tool", &json!({})));
    }
}
