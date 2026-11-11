//! The core's side of the protocol: one task that owns the runtime, answers requests in
//! the order they come, forwards the events of open threads and keeps the status fresh.

use std::{collections::HashMap, path::PathBuf, sync::Arc, time::Duration};

use anyhow::{Context, Result, anyhow, ensure};
use dusk_protocol::{
    client::{
        CatalogRequest, Event, ProjectRequest, Request, Response, SettingsRequest, Status, Switch,
        ThreadRequest, WorkRequest,
    },
    config::ApprovalPolicy,
    update::{Channel, InstallProgress},
};
use tokio::{
    sync::{mpsc, oneshot, watch},
    task::JoinHandle,
};
use tracing::warn;

use crate::{
    message::commands,
    project,
    runtime::{Runtime, ThreadHandle, ThreadInput},
};

/// How long a closing thread may take to stop its turn and finish.
const CLOSE_TIMEOUT: Duration = Duration::from_secs(10);
/// How often the status is read again for what changes in the background, such as a
/// model found over the network or an MCP server that settled.
const STATUS_PERIOD: Duration = Duration::from_millis(100);

pub struct Call {
    pub request: Request,
    pub reply: oneshot::Sender<Response>,
}

/// An open thread: where its input goes and the task that forwards its events.
struct OpenThread {
    input: ThreadInput,
    forward: JoinHandle<()>,
}

pub struct Server {
    runtime: Arc<Runtime>,
    /// The client's name, the source of the threads it creates.
    source: String,
    events: mpsc::UnboundedSender<Event>,
    status: Arc<watch::Sender<Status>>,
    threads: HashMap<String, OpenThread>,
}

impl Server {
    pub fn new(
        runtime: Runtime,
        source: String,
        events: mpsc::UnboundedSender<Event>,
    ) -> (Self, watch::Receiver<Status>) {
        let status = read_status(&runtime, None);
        let (sender, receiver) = watch::channel(status);
        let server = Self {
            runtime: Arc::new(runtime),
            source,
            events,
            status: Arc::new(sender),
            threads: HashMap::new(),
        };
        (server, receiver)
    }

    /// Answers calls until the client asks to shut down or goes away, then closes every
    /// thread and stops the runtime.
    pub async fn run(mut self, mut calls: mpsc::UnboundedReceiver<Call>) {
        let mut clock = tokio::time::interval(STATUS_PERIOD);
        clock.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let last = loop {
            tokio::select! {
                call = calls.recv() => match call {
                    None => break None,
                    Some(Call { request: Request::Shutdown, reply }) => break Some(reply),
                    Some(Call { request: Request::InstallUpdate { tag }, reply }) => {
                        self.install_update(tag, reply);
                    }
                    Some(Call { request, reply }) => {
                        let response = match self.handle(request).await {
                            Ok(response) => response,
                            Err(error) => Response::Error(format!("{error:#}")),
                        };
                        // The client reads the status once it has the answer.
                        self.refresh_status();
                        let _ = reply.send(response);
                    }
                },
                _ = clock.tick() => self.refresh_status(),
            }
        };
        let threads: Vec<String> = self.threads.keys().cloned().collect();
        for thread_id in threads {
            self.close(&thread_id).await;
        }
        match Arc::into_inner(self.runtime) {
            Some(runtime) => runtime.shutdown().await,
            None => warn!("an update install still holds the runtime; it stops without a shutdown"),
        }
        if let Some(reply) = last {
            let _ = reply.send(Response::Done);
        }
    }

    async fn handle(&mut self, request: Request) -> Result<Response> {
        match request {
            Request::Thread(request) => self.thread(request).await,
            Request::Settings(request) => self.settings(request).await,
            Request::Catalog(request) => self.catalog(request).await,
            Request::Work(request) => self.work(request).await,
            Request::Project(request) => self.project(request).await,
            Request::Command(command) => Ok(Response::Answer(Box::new(
                commands::answer(command, &self.runtime).await?,
            ))),
            Request::Voice(request) => self.voice(request).await,
            Request::SystemReport => Ok(Response::System(self.runtime.system_report().await?)),
            Request::InstallUpdate { .. } | Request::Shutdown => {
                Err(anyhow!("this request is answered by the loop"))
            }
        }
    }

    async fn thread(&mut self, request: ThreadRequest) -> Result<Response> {
        let runtime = Arc::clone(&self.runtime);
        Ok(match request {
            ThreadRequest::New { cwd } => {
                let handle = runtime.new_thread(&self.source, &cwd).await?;
                self.open(handle)
            }
            ThreadRequest::Resume { thread_id, cwd } => {
                self.close(&thread_id).await;
                let handle = runtime.resume_thread(&thread_id, &cwd).await?;
                self.open(handle)
            }
            ThreadRequest::Fork { summary, cwd } => {
                let handle = runtime.fork_thread(&self.source, &summary, &cwd).await?;
                self.open(handle)
            }
            ThreadRequest::List => Response::Threads(runtime.list_threads().await?),
            ThreadRequest::Messages { thread_id } => {
                Response::Messages(runtime.thread_messages(&thread_id).await?)
            }
            ThreadRequest::Tasks { thread_id } => {
                Response::Tasks(runtime.thread_tasks(&thread_id).await?)
            }
            ThreadRequest::LocalUnsent { thread_id } => {
                Response::LocalUnsent(runtime.local_unsent(&thread_id).await?)
            }
            ThreadRequest::ResendLocalUnsent { thread_id, id } => {
                runtime.resend_local_unsent(&thread_id, &id).await?;
                Response::Done
            }
            ThreadRequest::Send {
                thread_id,
                text,
                now,
            } => {
                let open = self.open_thread(&thread_id)?;
                let text = runtime
                    .keep_pastes(&thread_id, &text)
                    .await
                    .context("cannot keep the paste")?;
                match now {
                    true => open.input.send_now(text).await?,
                    false => open.input.send(text).await?,
                }
                Response::Done
            }
            ThreadRequest::Background { thread_id, call_id } => {
                runtime.send_to_background(&thread_id, &call_id).await?;
                Response::Done
            }
            ThreadRequest::RunCommand { thread_id, command } => {
                Response::Text(runtime.run_for_user(&thread_id, &command).await?)
            }
            ThreadRequest::Cancel { thread_id } => {
                self.open_thread(&thread_id)?.input.cancel().await?;
                Response::Done
            }
            ThreadRequest::Close { thread_id } => {
                self.close(&thread_id).await;
                Response::Done
            }
            ThreadRequest::StopAgent { thread_id } => {
                runtime.stop_agent(&thread_id).await?;
                Response::Done
            }
            ThreadRequest::Rename { thread_id, title } => {
                let title = title.trim();
                ensure!(
                    title.chars().count() <= 200,
                    "a title is at most 200 characters"
                );
                runtime.state().set_title(&thread_id, title).await?;
                Response::Done
            }
            ThreadRequest::Delete { thread_id } => {
                // Its turn ends first, so nothing writes to what is being erased.
                self.close(&thread_id).await;
                let runtime = &self.runtime;
                for folder in runtime.state().delete_thread(&thread_id).await? {
                    // Only plain names under the workspace root are taken.
                    let path = runtime.paths().workspace_folder(&folder)?;
                    match tokio::fs::remove_dir_all(&path).await {
                        Ok(()) => {}
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                        Err(error) => warn!("cannot remove a deleted chat's folder: {error}"),
                    }
                }
                Response::Done
            }
            ThreadRequest::Approve { id, decision } => {
                Response::Waiting(runtime.answer_approval(id, decision))
            }
            ThreadRequest::SetApprovals { thread_id, policy } => {
                runtime
                    .state()
                    .set_thread_approvals(&thread_id, policy)
                    .await?;
                Response::Done
            }
            ThreadRequest::GetApprovals { thread_id } => {
                Response::Approvals(runtime.state().thread_approvals(&thread_id).await?)
            }
        })
    }

    async fn voice(&self, request: dusk_protocol::voice::VoiceRequest) -> Result<Response> {
        use dusk_protocol::voice::VoiceRequest;
        Ok(match request {
            VoiceRequest::Transcribe { audio } => {
                Response::Text(self.runtime.transcribe(&audio).await?)
            }
            VoiceRequest::Speak { text } => Response::Card(self.runtime.speak(&text).await?),
        })
    }

    async fn settings(&self, request: SettingsRequest) -> Result<Response> {
        let runtime = &self.runtime;
        match request {
            SettingsRequest::ContextPolicy(policy) => {
                return Ok(Response::Number(runtime.set_context_policy(policy)?));
            }
            SettingsRequest::MaxAgentsAtWork(limit) => runtime.set_max_agents_at_work(limit)?,
            SettingsRequest::IdleScreenMinutes(minutes) => {
                runtime.set_idle_screen_minutes(minutes)?;
                let saved = runtime.config().current().appearance().idle_screen_minutes;
                return Ok(Response::Number(saved.into()));
            }
            SettingsRequest::MaxGoalRounds(rounds) => runtime.set_max_goal_rounds(rounds)?,
            SettingsRequest::MemoryChars(chars) => runtime.set_memory_chars(chars)?,
            SettingsRequest::WorkHistoryChars(chars) => {
                runtime.set_work_history_chars(chars)?;
                let saved = runtime.config().current().work_history().max_chars;
                return Ok(Response::Number(saved as u64));
            }
            SettingsRequest::Switch { switch, on } => self.switch(switch, on)?,
            SettingsRequest::BranchPrefix(prefix) => runtime.set_branch_prefix(&prefix)?,
            SettingsRequest::NextMode => return Ok(Response::Text(runtime.next_mode()?)),
            SettingsRequest::Mode(name) => runtime.set_mode(&name)?,
            SettingsRequest::Connect {
                kind,
                name,
                base_url,
                api_key,
            } => {
                let api_key = api_key.map(|key| key.0);
                let name = runtime.connect_provider(kind, name, base_url, api_key)?;
                return Ok(Response::Text(name));
            }
            SettingsRequest::RemoveProvider { name } => runtime.remove_provider(&name)?,
            SettingsRequest::RemoveAllProviders => runtime.remove_all_providers()?,
            SettingsRequest::SetSecret { name, value } => {
                runtime.set_secret(&name, &value.0).await?
            }
            SettingsRequest::DeleteSecret { name } => runtime.delete_secret(&name).await?,
            SettingsRequest::Voice(voice) => runtime.set_voice(voice)?,
            SettingsRequest::PluginEnabled { name, enabled } => {
                runtime.set_plugin_enabled(&name, enabled)?;
            }
            SettingsRequest::PluginSecret { plugin, key, value } => {
                runtime.set_plugin_secret(&plugin, &key, &value.0)?;
            }
        }
        Ok(Response::Done)
    }

    fn switch(&self, switch: Switch, on: bool) -> Result<()> {
        let runtime = &self.runtime;
        match switch {
            Switch::Agent(name) => runtime.set_agent_enabled(&name, on),
            Switch::Tool(name) => runtime.set_tool_enabled(&name, on),
            Switch::Skill(name) => runtime.set_skill_enabled(&name, on),
            Switch::Mcp(name) => runtime.set_mcp_enabled(&name, on),
            Switch::Memory => runtime.set_memory_enabled(on),
            Switch::WorkHistory => runtime.set_work_history_enabled(on),
            Switch::DevChannel => runtime.set_update_channel(match on {
                true => Channel::Dev,
                false => Channel::Stable,
            }),
            Switch::Approvals(ApprovalPolicy::Sandbox) if on => {
                ensure!(
                    runtime.platform().sandbox,
                    "this system has no sandbox for commands"
                );
                runtime.set_approvals(ApprovalPolicy::Sandbox)
            }
            Switch::Approvals(policy) => runtime.set_approvals(match on {
                true => policy,
                false => ApprovalPolicy::Ask,
            }),
            Switch::Logging => runtime.set_logging_enabled(on),
            Switch::Notifications => runtime.set_notifications(on),
            Switch::SoundNotifications => runtime.set_sound_notifications(on),
        }
    }

    async fn catalog(&self, request: CatalogRequest) -> Result<Response> {
        let runtime = &self.runtime;
        Ok(match request {
            CatalogRequest::Skill { name } => Response::Node(runtime.skill(&name)?),
            CatalogRequest::SaveSkill { name, text } => {
                Response::Node(runtime.save_skill(&name, &text)?)
            }
            CatalogRequest::Agent { name } => Response::Node(runtime.agent(&name)?),
            CatalogRequest::SaveAgent { name, text } => {
                Response::Node(runtime.save_agent(&name, &text).await?)
            }
            CatalogRequest::McpServers => Response::Mcp(runtime.mcp_servers()),
            CatalogRequest::Snippets => Response::Snippets(runtime.mentions().snippets()?),
            CatalogRequest::SaveSnippet { name, text } => {
                runtime.mentions().save_snippet(&name, &text)?;
                Response::Done
            }
            CatalogRequest::Profile => Response::Profile(runtime.mentions().profile()?),
            CatalogRequest::Secrets => Response::Secrets(runtime.secrets_info().await),
            CatalogRequest::Voice => Response::Voice(runtime.voice_settings()),
            CatalogRequest::SaveProfile(profile) => {
                runtime.mentions().save_profile(&profile)?;
                Response::Done
            }
            CatalogRequest::McpServerText { name } => {
                Response::Text(runtime.mcp_server_text(&name)?)
            }
            CatalogRequest::SaveMcpServer { old, text } => {
                Response::Name(runtime.save_mcp_server(old.as_deref(), &text)?)
            }
            CatalogRequest::RestartMcp { name } => {
                runtime.restart_mcp(&name);
                Response::Done
            }
            CatalogRequest::SignInMcp { name } => {
                Response::Text(runtime.sign_in_mcp(&name).await?.to_string())
            }
        })
    }

    async fn work(&self, request: WorkRequest) -> Result<Response> {
        let runtime = &self.runtime;
        Ok(match request {
            WorkRequest::Jobs { thread_id } => Response::Jobs(runtime.jobs(&thread_id).await?),
            WorkRequest::JobOutput { thread_id, job_id } => {
                Response::Text(runtime.job_output(&thread_id, &job_id).await?)
            }
            WorkRequest::StopJob { thread_id, job_id } => {
                Response::Text(runtime.stop_job(&thread_id, &job_id).await?)
            }
            WorkRequest::Plan { thread_id } => Response::Plan(runtime.plan(&thread_id).await?),
            WorkRequest::Diff { thread_id, path } => {
                Response::Diff(runtime.chat_diff(&thread_id, path.as_deref()).await?)
            }
            WorkRequest::ChangedFiles { thread_id } => {
                Response::Changes(runtime.changed_files(&thread_id).await?)
            }
            WorkRequest::SavePlan { thread_id, text } => {
                runtime.save_plan(&thread_id, &text).await?;
                Response::Done
            }
            WorkRequest::PinSource {
                thread_id,
                id,
                pinned,
            } => {
                runtime.pin_source(&thread_id, &id, pinned).await?;
                Response::Done
            }
            WorkRequest::RemoveSource { thread_id, id } => {
                runtime.remove_source(&thread_id, &id).await?;
                Response::Done
            }
        })
    }

    async fn project(&self, request: ProjectRequest) -> Result<Response> {
        let state = self.runtime.state();
        Ok(match request {
            ProjectRequest::List => Response::Projects(state.list_projects().await?),
            ProjectRequest::Save {
                id,
                name,
                description,
                dir,
            } => Response::Project(project::save(state, id, &name, &description, &dir).await?),
            ProjectRequest::Env { id } => Response::Names(project::env::names(state, &id).await?),
            ProjectRequest::SetEnv { id, name, value } => {
                Response::Names(project::env::set(state, &id, &name, &value.0).await?)
            }
            ProjectRequest::RemoveEnv { id, name } => {
                Response::Names(project::env::remove(state, &id, &name).await?)
            }
            ProjectRequest::Remove { id } => {
                state.remove_project(&id).await?;
                Response::Done
            }
            ProjectRequest::Git { id } => {
                Response::Git(project::git(&project::find(state, &id).await?).await?)
            }
            ProjectRequest::Diff { id, path } => Response::Diff(
                project::diff(&project::find(state, &id).await?.dir, path.as_deref()).await?,
            ),
            ProjectRequest::SwitchBranch { id, branch } => {
                project::switch_branch(&project::find(state, &id).await?, &branch).await?;
                Response::Done
            }
            ProjectRequest::Files { id, path } => {
                Response::Files(project::files(&project::find(state, &id).await?, &path).await?)
            }
            ProjectRequest::Outputs => {
                Response::Outputs(project::outputs(self.output_roots()).await?)
            }
            ProjectRequest::DeleteOutput { path } => {
                project::delete_output(&path, &self.output_roots()).await?;
                Response::Done
            }
            ProjectRequest::ReadOutput { path } => {
                let mut roots = self.output_roots();
                roots.extend(state.list_projects().await?.into_iter().map(|p| p.dir));
                Response::Text(project::read_output(&path, &roots).await?)
            }
        })
    }

    fn output_roots(&self) -> Vec<PathBuf> {
        let paths = self.runtime.paths();
        vec![paths.workspace_root().to_path_buf(), paths.artifacts_dir()]
    }

    /// Keeps the thread's input and forwards its events until its task ends.
    fn open(&mut self, handle: ThreadHandle) -> Response {
        let ThreadHandle {
            thread_id,
            input,
            mut output,
        } = handle;
        let events = self.events.clone();
        let id = thread_id.clone();
        let forward = tokio::spawn(async move {
            while let Some(event) = output.next_event().await {
                let event = Event::Thread {
                    thread_id: id.clone(),
                    event: Box::new(event),
                };
                if events.send(event).is_err() {
                    return;
                }
            }
            let _ = events.send(Event::Ended { thread_id: id });
        });
        self.threads
            .insert(thread_id.clone(), OpenThread { input, forward });
        Response::Opened { thread_id }
    }

    fn open_thread(&self, thread_id: &str) -> Result<&OpenThread> {
        self.threads
            .get(thread_id)
            .with_context(|| format!("thread {thread_id} is not open"))
    }

    /// Stops the thread's turn and waits for its task to end, so it can be opened again.
    async fn close(&mut self, thread_id: &str) {
        let Some(OpenThread { input, forward }) = self.threads.remove(thread_id) else {
            return;
        };
        if forward.is_finished() {
            return;
        }
        // Cancelling a thread that already ended has nothing to stop.
        let _ = input.cancel().await;
        drop(input);
        let _ = tokio::time::timeout(CLOSE_TIMEOUT, forward).await;
    }

    /// Installs a release in the background; the reply comes once it is done, and the
    /// status shows how far it got meanwhile.
    fn install_update(&self, tag: String, reply: oneshot::Sender<Response>) {
        let runtime = Arc::clone(&self.runtime);
        let status = Arc::clone(&self.status);
        status.send_modify(|status| status.install = Some(InstallProgress::default()));
        tokio::spawn(async move {
            let (sender, mut progress) = watch::channel(InstallProgress::default());
            let install = runtime.install_update_with_progress(&tag, sender);
            tokio::pin!(install);
            let result: Result<PathBuf> = loop {
                tokio::select! {
                    result = &mut install => break result,
                    changed = progress.changed() => {
                        if changed.is_ok() {
                            let now = *progress.borrow_and_update();
                            status.send_modify(|status| status.install = Some(now));
                        }
                    }
                }
            };
            let now = *progress.borrow();
            status.send_modify(|status| status.install = result.is_ok().then_some(now));
            let response = match result {
                Ok(path) => Response::Path(path),
                Err(error) => Response::Error(format!("{error:#}")),
            };
            let _ = reply.send(response);
        });
    }

    fn refresh_status(&self) {
        let mut status = read_status(&self.runtime, None);
        self.status.send_if_modified(|old| {
            // The install task writes its own field; it is never read here.
            status.install = old.install;
            let changed = *old != status;
            if changed {
                *old = status;
            }
            changed
        });
    }
}

fn read_status(runtime: &Runtime, install: Option<InstallProgress>) -> Status {
    let config = runtime.config().current();
    Status {
        model: runtime.model().map(|choice| choice.name()),
        discovering_model: runtime.discovering_model(),
        input_limit: runtime.input_limit(),
        context_policy: runtime.context_policy(),
        effort: config.effort(),
        mode: config.mode().to_owned(),
        approvals: config.approvals(),
        max_goal_rounds: config.limits().max_goal_rounds,
        idle_screen_minutes: config.appearance().idle_screen_minutes,
        notifications: config.notifications(),
        sound_notifications: config.sound_notifications(),
        // Read before the statuses: a server that settles in between still shows.
        mcp_starting: runtime.mcp_starting(),
        mcp: runtime.mcp_statuses(),
        plugins: runtime.plugins(),
        available_update: runtime.available_update(),
        install,
        updated_builtin: runtime.updated_builtin().to_vec(),
        artifacts_dir: runtime.paths().artifacts_dir(),
    }
}
