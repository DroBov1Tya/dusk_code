//! Platforms: programs of plugins that bring the user in from somewhere else, such as a
//! chat bot. The core keeps each running, starts it again if it ends, and gives it a
//! thread of its own, the newest one it had or a new one.
//!
//! They talk in lines of JSON. The core sends what the user should see:
//!
//! - `{"type": "answer", "text": "…"}` — the agent's final answer, in Markdown;
//! - `{"type": "question", "question": "…", "options": […], "multiple": false}`;
//! - `{"type": "approval", "id": 3, "text": "the agent wants to run: …"}`;
//! - `{"type": "reply", "text": "…"}` — a short answer to a command, or a note;
//! - `{"type": "menu", "command": "mode", "title": "…", "options": […]}`;
//! - `{"type": "image", "path": "/…", "mime_type": "image/png", "caption": "…"}` — an
//!   image card the agent showed, its file to send on;
//! - `{"type": "working", "on": true}`.
//!
//! The platform sends what the user did:
//!
//! - `{"type": "message", "text": "…"}` — a message, or the answer to an open question;
//! - `{"type": "chosen", "options": [0, 2]}` — options picked for the open question;
//! - `{"type": "approval", "id": 3, "allow": true}`;
//! - `{"type": "command", "name": "status", "argument": null}`, where the name is one of
//!   stop, resume, status, approve, deny, mode, model, compact, goal and new;
//! - `{"type": "status", "text": "…"}` — what `/settings` shows about it, such as the code
//!   to pair a bot with.
//!
//! Whoever reaches the platform acts as the user, so the platform decides who that is.

use std::{collections::BTreeMap, path::PathBuf, process::Stdio, sync::Arc, time::Duration};

use anyhow::{Context, Result};
use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Serialize};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{ChildStdin, Command},
    task::JoinHandle,
};
use tracing::{info, warn};

use crate::{
    providers::ModelEvent,
    runtime::{Launcher, ThreadEvent},
    state::{Goal, GoalStatus, TurnStatus, artifacts::ArtifactStore},
    tools::{ApprovalRequest, Decision, Question},
};

/// Waits before a platform that ended is started again, doubled up to the last.
const RESTART_WAITS: [u64; 4] = [2, 10, 30, 60];
/// A line longer than this is no message of the protocol.
const MAX_LINE_BYTES: usize = 1024 * 1024;

/// A platform of a plugin that is on.
#[derive(Clone)]
pub struct PlatformSpec {
    pub plugin: String,
    pub command: Vec<String>,
    pub folder: PathBuf,
    pub env: Vec<(String, String)>,
}

#[derive(Default)]
pub struct Platforms {
    launcher: std::sync::OnceLock<Launcher>,
    /// Each running platform with what it was started from, to tell a change.
    running: Mutex<BTreeMap<String, (String, JoinHandle<()>)>>,
    statuses: Arc<RwLock<BTreeMap<String, String>>>,
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ToPlatform<'a> {
    Answer {
        text: &'a str,
    },
    Question {
        question: &'a str,
        options: &'a [String],
        multiple: bool,
    },
    Approval {
        id: u64,
        text: String,
    },
    Reply {
        text: &'a str,
    },
    Menu {
        command: &'a str,
        title: String,
        options: Vec<String>,
    },
    Image {
        path: String,
        mime_type: &'a str,
        caption: Option<String>,
    },
    Working {
        on: bool,
    },
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum FromPlatform {
    Message {
        text: String,
    },
    Chosen {
        options: Vec<usize>,
    },
    Approval {
        id: u64,
        allow: bool,
    },
    Command {
        name: String,
        argument: Option<String>,
    },
    Status {
        text: String,
    },
}

impl Platforms {
    /// Gives the platforms the core to start threads with; set once at start.
    pub fn attach(&self, launcher: Launcher) {
        let _ = self.launcher.set(launcher);
    }

    pub fn status(&self, plugin: &str) -> Option<String> {
        self.statuses.read().get(plugin).cloned()
    }

    /// Starts the platforms that are new or changed and stops those that are gone.
    pub fn set(&self, specs: Vec<PlatformSpec>) {
        let Some(launcher) = self.launcher.get() else {
            return;
        };
        let wanted: BTreeMap<String, (String, PlatformSpec)> = specs
            .into_iter()
            .map(|spec| {
                let started_from = format!("{:?} {:?}", spec.command, spec.env);
                (spec.plugin.clone(), (started_from, spec))
            })
            .collect();
        let mut running = self.running.lock();
        running.retain(|name, (started_from, task)| {
            let keep = wanted
                .get(name)
                .is_some_and(|(wanted, _)| wanted == started_from);
            if !keep {
                task.abort();
                self.statuses.write().remove(name);
                info!("platform of plugin {name} stops");
            }
            keep
        });
        for (name, (started_from, spec)) in wanted {
            if running.contains_key(&name) {
                continue;
            }
            info!("platform of plugin {name} starts");
            let task = tokio::spawn(keep_running(
                launcher.clone(),
                spec,
                Arc::clone(&self.statuses),
            ));
            running.insert(name, (started_from, task));
        }
    }

    pub fn stop_all(&self) {
        for (_, (_, task)) in std::mem::take(&mut *self.running.lock()) {
            task.abort();
        }
    }
}

async fn keep_running(
    launcher: Launcher,
    spec: PlatformSpec,
    statuses: Arc<RwLock<BTreeMap<String, String>>>,
) {
    let mut attempt = 0usize;
    loop {
        let ended = match Session::run(&launcher, &spec, &statuses).await {
            Ok(()) => "the platform ended".to_owned(),
            Err(error) => format!("the platform failed: {error:#}"),
        };
        warn!("plugin {}: {ended}", spec.plugin);
        statuses.write().insert(spec.plugin.clone(), ended);
        let wait = RESTART_WAITS[attempt.min(RESTART_WAITS.len() - 1)];
        attempt = attempt.saturating_add(1);
        tokio::time::sleep(Duration::from_secs(wait)).await;
    }
}

/// One run of a platform's program, bridged to its thread.
struct Session<'a> {
    launcher: &'a Launcher,
    spec: &'a PlatformSpec,
    stdin: ChildStdin,
    thread: crate::runtime::ThreadHandle,
    /// The answer so far: the text since the last tool call.
    answer: String,
    working: bool,
    question: Option<Question>,
    approvals: Vec<ApprovalRequest>,
    /// The request the user was shown, which approve and deny answer.
    shown: Option<u64>,
    goal: Option<Goal>,
}

impl<'a> Session<'a> {
    async fn run(
        launcher: &'a Launcher,
        spec: &'a PlatformSpec,
        statuses: &RwLock<BTreeMap<String, String>>,
    ) -> Result<()> {
        let (program, arguments) = spec.command.split_first().context("no command")?;
        let mut child = Command::new(program)
            .args(arguments)
            .current_dir(&spec.folder)
            .envs(spec.env.iter().map(|(key, value)| (key, value)))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .with_context(|| format!("cannot start {program}"))?;
        let stdin = child.stdin.take().context("no stdin")?;
        let mut lines = BufReader::new(child.stdout.take().context("no stdout")?).lines();
        let thread = open_thread(launcher, &spec.plugin, None).await?;
        let mut session = Session {
            launcher,
            spec,
            stdin,
            thread,
            answer: String::new(),
            working: false,
            question: None,
            approvals: Vec::new(),
            shown: None,
            goal: None,
        };
        loop {
            tokio::select! {
                line = lines.next_line() => {
                    let Some(line) = line.context("cannot read the platform")? else {
                        return Ok(());
                    };
                    if line.len() > MAX_LINE_BYTES {
                        continue;
                    }
                    match serde_json::from_str::<FromPlatform>(&line) {
                        Ok(FromPlatform::Status { text }) => {
                            statuses.write().insert(spec.plugin.clone(), text);
                        }
                        Ok(input) => session.take(input).await?,
                        Err(error) => warn!("plugin {}: not a message: {error}", spec.plugin),
                    }
                }
                event = session.thread.output.next_event() => match event {
                    Some(event) => session.show(event).await?,
                    // The thread ended, such as after an error: the platform gets a new one.
                    None => {
                        let id = session.thread.thread_id.clone();
                        session.thread = open_thread(launcher, &spec.plugin, Some(&id)).await?;
                    }
                },
            }
        }
    }

    async fn send(&mut self, message: ToPlatform<'_>) -> Result<()> {
        let mut line = serde_json::to_string(&message)?;
        line.push('\n');
        self.stdin
            .write_all(line.as_bytes())
            .await
            .context("cannot write to the platform")
    }

    async fn reply(&mut self, text: &str) -> Result<()> {
        self.send(ToPlatform::Reply { text }).await
    }

    /// Passes on what the user should see of a thread event.
    async fn show(&mut self, event: ThreadEvent) -> Result<()> {
        match event {
            ThreadEvent::TurnStarted(_) => {
                self.answer.clear();
                self.working = true;
                self.send(ToPlatform::Working { on: true }).await
            }
            ThreadEvent::Model(ModelEvent::Text(text)) => {
                self.answer.push_str(&text);
                Ok(())
            }
            ThreadEvent::ToolStarted(_) => {
                self.answer.clear();
                Ok(())
            }
            ThreadEvent::ToolFinished { result, .. } => {
                let store = ArtifactStore::new(self.launcher.paths.artifacts_dir());
                // Pictures for the model, such as screenshots, stay in the thread; a sound is
                // for a client that plays it, and no platform takes one yet.
                let pictures = result.artifacts.iter().filter(|card| {
                    !card.for_model && card.kind == dusk_protocol::tools::CardKind::Image
                });
                for card in pictures {
                    let caption = [&card.title, &card.description]
                        .into_iter()
                        .flatten()
                        .cloned()
                        .collect::<Vec<_>>()
                        .join("\n");
                    self.send(ToPlatform::Image {
                        path: store.path(card)?.display().to_string(),
                        mime_type: &card.mime_type,
                        caption: (!caption.is_empty()).then_some(caption),
                    })
                    .await?;
                }
                Ok(())
            }
            ThreadEvent::TurnFinished(turn) => {
                self.working = false;
                self.send(ToPlatform::Working { on: false }).await?;
                let answer = std::mem::take(&mut self.answer);
                match turn.status {
                    TurnStatus::Completed if !answer.trim().is_empty() => {
                        self.send(ToPlatform::Answer { text: &answer }).await
                    }
                    TurnStatus::Failed => {
                        let reason = turn.error_message.as_deref().unwrap_or("no reason given");
                        self.reply(&format!("The turn failed: {reason}")).await
                    }
                    TurnStatus::Interrupted => self.reply("Stopped.").await,
                    _ => Ok(()),
                }
            }
            ThreadEvent::Question(question) => {
                self.send(ToPlatform::Question {
                    question: &question.question,
                    options: &question.options,
                    multiple: question.multiple,
                })
                .await?;
                self.question = Some(question);
                Ok(())
            }
            ThreadEvent::Approval(request) => {
                self.approvals.push(request);
                self.show_approval().await
            }
            ThreadEvent::ApprovalClosed { id } => {
                self.approvals.retain(|request| request.id != id);
                if self.shown == Some(id) {
                    self.shown = None;
                }
                self.show_approval().await
            }
            // An agent's request waits on the user as well; the rest stays in its thread.
            ThreadEvent::Subagent { event, .. } => match *event {
                event @ (ThreadEvent::Approval(_)
                | ThreadEvent::ApprovalClosed { .. }
                | ThreadEvent::Subagent { .. }) => Box::pin(self.show(event)).await,
                _ => Ok(()),
            },
            ThreadEvent::Goal { goal, note, .. } => {
                // Proposed criteria wait on the user: approved, or sent back to change.
                let proposed = goal
                    .as_ref()
                    .is_some_and(|goal| !goal.approved && !goal.criteria.is_empty());
                self.goal = goal;
                if proposed {
                    return self
                        .send(ToPlatform::Menu {
                            command: "goal",
                            title: note,
                            options: vec!["approve".into(), "change".into()],
                        })
                        .await;
                }
                if note.is_empty() {
                    return Ok(());
                }
                self.reply(&note).await
            }
            ThreadEvent::Rejected { reason } => self.reply(&reason).await,
            ThreadEvent::Failed(error) => self.reply(&format!("The thread stopped: {error}")).await,
            _ => Ok(()),
        }
    }

    /// Shows the oldest waiting request, once.
    async fn show_approval(&mut self) -> Result<()> {
        if self.shown.is_some() {
            return Ok(());
        }
        let Some(request) = self.approvals.first() else {
            return Ok(());
        };
        let (id, text) = (request.id, request.request.describe("the agent"));
        self.shown = Some(id);
        self.send(ToPlatform::Approval { id, text }).await
    }

    /// Does what the user did on the platform, as if at the keyboard.
    async fn take(&mut self, input: FromPlatform) -> Result<()> {
        match input {
            FromPlatform::Message { text } => match self.question.take() {
                Some(question) => self.thread.input.send(question.answer(&[], &text)).await,
                None if text.trim_start().starts_with('/') => {
                    self.reply("Commands go by the platform's own menu; /help lists them.")
                        .await
                }
                None => self.thread.input.send(text).await,
            },
            FromPlatform::Chosen { options } => match self.question.take() {
                Some(question) => self.thread.input.send(question.answer(&options, "")).await,
                None => self.reply("That question is closed.").await,
            },
            FromPlatform::Approval { id, allow } => self.decide(Some(id), allow).await,
            FromPlatform::Command { name, argument } => self.command(&name, argument).await,
            FromPlatform::Status { .. } => Ok(()),
        }
    }

    async fn decide(&mut self, id: Option<u64>, allow: bool) -> Result<()> {
        let Some(id) = id.filter(|id| self.approvals.iter().any(|request| request.id == *id))
        else {
            return self.reply("Nothing is waiting for that approval.").await;
        };
        let decision = if allow {
            Decision::Once
        } else {
            Decision::Deny
        };
        self.launcher.approvals.answer(id, decision);
        Ok(())
    }

    async fn command(&mut self, name: &str, argument: Option<String>) -> Result<()> {
        let input = self.thread.input.clone();
        match (name, argument) {
            ("stop", _) if self.working => {
                input.cancel().await?;
                self.reply("Stopping.").await
            }
            ("stop", _) => self.reply("The agent is not working.").await,
            ("resume", _) => {
                let paused = self
                    .goal
                    .as_ref()
                    .is_some_and(|goal| goal.status == GoalStatus::Paused);
                match (paused, self.working) {
                    (true, _) => input.send("/goal resume").await,
                    (false, true) => self.reply("The agent is already working.").await,
                    (false, false) => input.send("Continue where you stopped.").await,
                }
            }
            ("status", _) => {
                let status = self.status();
                self.reply(&status).await
            }
            ("approve", _) => self.decide(self.shown, true).await,
            ("deny", _) => self.decide(self.shown, false).await,
            ("mode", None) => {
                let config = self.launcher.config.current();
                let title = format!("Mode: {}", config.mode());
                let options =
                    crate::catalog::modes::Modes::new(&self.launcher.paths.modes_dir()).list();
                self.send(ToPlatform::Menu {
                    command: "mode",
                    title,
                    options,
                })
                .await
            }
            ("mode", Some(mode)) => {
                let modes =
                    crate::catalog::modes::Modes::new(&self.launcher.paths.modes_dir()).list();
                if !modes.contains(&mode) {
                    return self.reply(&format!("No mode is named {mode}.")).await;
                }
                let name = mode.clone();
                self.launcher
                    .config
                    .update(|config| config.set_mode(name))?;
                self.reply(&format!("Mode: {mode}")).await
            }
            ("model", None) => {
                let model = self.model_name();
                self.reply(&format!(
                    "Model: {model}. Send /model provider/model to switch."
                ))
                .await
            }
            ("model", Some(model)) => input.send(format!("/model {model}")).await,
            ("compact", instructions) => {
                self.reply("Compacting the context.").await?;
                input
                    .send(format!("/compact {}", instructions.unwrap_or_default()))
                    .await
            }
            ("goal", None) => {
                let line = self.goal_line();
                self.reply(&line).await
            }
            // What the menu of proposed criteria sends.
            ("goal", Some(choice)) if choice == "approve" => match &self.goal {
                Some(goal) if !goal.approved => {
                    input.send(format!("/goal approve {}", goal.revision)).await
                }
                _ => self.reply("No criteria wait for approval.").await,
            },
            ("goal", Some(choice)) if choice == "change" => {
                self.reply("Send what to change in the criteria.").await
            }
            ("goal", Some(text)) => input.send(format!("/goal {text}")).await,
            ("new", _) => {
                self.thread = new_thread(self.launcher, &self.spec.plugin).await?;
                self.reply("A new thread.").await
            }
            (other, _) => self.reply(&format!("Unknown command {other}.")).await,
        }
    }

    fn model_name(&self) -> String {
        self.launcher
            .model
            .current()
            .map_or_else(|| "none".into(), |choice| choice.name())
    }

    fn status(&self) -> String {
        let agent = if self.working { "working" } else { "idle" };
        let waits = match (self.question.is_some(), self.approvals.len()) {
            (true, _) => "\nWaits for: your answer to a question".to_owned(),
            (false, 0) => String::new(),
            (false, count) => format!("\nWaits for: {count} approval(s)"),
        };
        format!(
            "Agent: {agent}\nMode: {} \u{00B7} model: {}\n{}{waits}",
            self.launcher.config.current().mode(),
            self.model_name(),
            self.goal_line()
        )
    }

    fn goal_line(&self) -> String {
        match &self.goal {
            Some(goal) => {
                let status = match goal.status {
                    GoalStatus::Active if goal.approved => "active",
                    GoalStatus::Active => "draft",
                    GoalStatus::Paused => "paused",
                    GoalStatus::Done => "done",
                };
                format!("Goal ({status}, round {}): {}", goal.rounds, goal.text)
            }
            None => "Goal: none".into(),
        }
    }
}

/// What a platform's threads are recorded as having come from.
fn source(plugin: &str) -> String {
    format!("plugin:{plugin}")
}

/// The platform's newest thread, other than one that just ended, or a new one.
async fn open_thread(
    launcher: &Launcher,
    plugin: &str,
    ended: Option<&str>,
) -> Result<crate::runtime::ThreadHandle> {
    let source = source(plugin);
    let newest = launcher
        .state
        .list_threads()
        .await?
        .into_iter()
        .find(|thread| thread.source == source && Some(thread.id.as_str()) != ended);
    match newest {
        Some(thread) => {
            launcher
                .resume_thread(&thread.id, launcher.paths.workspace_root())
                .await
        }
        None => new_thread(launcher, plugin).await,
    }
}

async fn new_thread(launcher: &Launcher, plugin: &str) -> Result<crate::runtime::ThreadHandle> {
    let cwd = launcher.paths.workspace_root().to_owned();
    crate::init::create_private_dir(&cwd)?;
    launcher.new_thread(&source(plugin), &cwd).await
}
