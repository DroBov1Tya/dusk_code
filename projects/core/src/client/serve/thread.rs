//! Requests about one thread: opening, sending, pinning, closing.

use super::publish::{publish, send_to};
use super::{CLOSE_TIMEOUT, OpenThread, Running, Server};
use super::{ClientId, Shown};
use super::{thread_pulls, unread};
use crate::runtime::ThreadHandle;
use anyhow::{Context, Result, bail, ensure};
use dusk_protocol::client::{Event, Response, ThreadRequest};
use dusk_protocol::config::ApprovalPolicy;
use dusk_protocol::thread::{ThreadEvent, TurnWait};
use parking_lot::Mutex;
use std::{
    collections::HashSet,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
use tracing::warn;

/// For a client bound to chats that asks to open one with no folder stored.
const NO_FOLDER: &str = "this chat cannot be opened from this client";

/// What a chat's event does to the suggestion kept for a client that joins: a new one,
/// none any more, or (`None`) nothing. A subagent's event comes wrapped and changes
/// nothing: only the chat has a user.
pub(super) fn suggestion_after(event: &ThreadEvent) -> Option<Option<String>> {
    match event {
        ThreadEvent::NextSuggested { text } => Some(Some(text.clone())),
        event if event.ends_suggestion() => Some(None),
        _ => None,
    }
}

impl Server {
    pub(super) async fn thread(&mut self, request: ThreadRequest) -> Result<Response> {
        let runtime = Arc::clone(&self.runtime);
        Ok(match request {
            ThreadRequest::New { cwd, approvals } => {
                let handle = runtime.new_thread(self.source(), &cwd).await?;
                self.open_with(handle, approvals).await?
            }
            ThreadRequest::NewTemporary { cwd, approvals } => {
                let handle = runtime.new_temporary_thread(self.source(), &cwd).await?;
                self.open_with(handle, approvals).await?
            }
            ThreadRequest::Resume { thread_id, cwd } => {
                // A thread another client keeps open runs on; this one just joins it.
                if self
                    .threads
                    .get(&thread_id)
                    .is_some_and(|open| !open.forward.is_finished())
                {
                    // Read before the lock, which no wait may hold.
                    let failure = runtime.last_failure(&thread_id).await?;
                    let agents = runtime.agents_at_work(&thread_id).await?;
                    let goal = runtime.goal(&thread_id).await?;
                    let Some(open) = self.threads.get_mut(&thread_id) else {
                        anyhow::bail!("thread has ended");
                    };
                    open.clients.insert(self.caller);
                    open.orphaned = None;
                    // This client missed the turn's start; it learns the turn runs
                    // without a fold boundary, or waits, with the question it waits on.
                    // The lock keeps it before the end. With no turn running, it sees why
                    // the newest one failed, as the others did.
                    // In the forwarding task's order: the suggestion, then the turn.
                    let suggested = open.suggested.lock();
                    let running = open.running.lock();
                    let events = match (running.clone(), failure) {
                        (Some(Running { turn, question }), _) if turn.waiting.is_some() => {
                            let question = question.map(ThreadEvent::Question);
                            [Some(ThreadEvent::TurnWaiting(turn)), question]
                        }
                        (Some(Running { turn, .. }), _) => {
                            [Some(ThreadEvent::TurnRunning(turn)), None]
                        }
                        (None, Some(failure)) => [Some(ThreadEvent::TurnFailed(failure)), None],
                        (None, None) => [None, None],
                    };
                    let agents = agents.then_some(ThreadEvent::AgentsWorking(true));
                    // The thread told its goal when it opened, before this client came.
                    let goal = goal.map(|goal| ThreadEvent::Goal {
                        goal: Some(goal),
                        note: String::new(),
                        started: None,
                    });
                    let suggestion = suggested
                        .clone()
                        .map(|text| ThreadEvent::NextSuggested { text });
                    for event in events
                        .into_iter()
                        .chain([goal, agents, suggestion])
                        .flatten()
                    {
                        send_to(
                            &self.subscribers,
                            self.caller,
                            Event::Thread {
                                thread_id: thread_id.clone(),
                                event: Box::new(event),
                            },
                        );
                    }
                    return Ok(Response::Opened { thread_id });
                }
                // Whoever kept the ended thread open keeps it after it starts again.
                let members = self
                    .threads
                    .get(&thread_id)
                    .map(|open| open.clients.clone())
                    .unwrap_or_default();
                let cwd = self.resume_folder(&thread_id, cwd).await?;
                self.close(&thread_id).await;
                let handle = runtime.resume_thread(&thread_id, &cwd).await?;
                let opened = self.open(handle);
                if let Some(open) = self.threads.get_mut(&thread_id) {
                    open.clients.extend(members);
                }
                opened
            }
            ThreadRequest::Fork { summary, cwd } => {
                let handle = runtime.fork_thread(self.source(), &summary, &cwd).await?;
                self.open(handle)
            }
            ThreadRequest::ForkAt {
                thread_id,
                message_id,
                cwd,
            } => {
                let handle = runtime
                    .fork_thread_at(self.source(), &thread_id, &message_id, &cwd)
                    .await?;
                self.open(handle)
            }
            ThreadRequest::List => Response::Threads(runtime.list_threads().await?),
            ThreadRequest::Search { text } => {
                let within = self.bound();
                Response::Found(runtime.search_chats(&text, within.as_deref()).await?)
            }
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
                    .keep_pastes(&thread_id, &text, self.host_files())
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
            // Answered beside the loop, by the server.
            ThreadRequest::RunCommand { .. } | ThreadRequest::StopRun { .. } => {
                bail!("a command the user runs is answered beside the loop")
            }
            ThreadRequest::Cancel { thread_id } => {
                self.open_thread(&thread_id)?.input.cancel().await?;
                Response::Done
            }
            ThreadRequest::Close { thread_id } => {
                self.leave(&thread_id).await;
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
                runtime
                    .state_of(&thread_id)
                    .await
                    .set_title(&thread_id, title)
                    .await?;
                Response::Done
            }
            ThreadRequest::Delete { thread_id } => {
                // Its turn ends first, so nothing writes to what is being erased.
                self.close(&thread_id).await;
                let runtime = &self.runtime;
                let (state, paths) = runtime.places_of(&thread_id).await;
                for folder in state.delete_thread(&thread_id).await? {
                    // Only plain names under the workspace root are taken.
                    let path = paths.workspace_folder(&folder)?;
                    match tokio::fs::remove_dir_all(&path).await {
                        Ok(()) => {}
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                        Err(error) => warn!("cannot remove a deleted chat's folder: {error}"),
                    }
                }
                Response::Done
            }
            ThreadRequest::Approve {
                thread_id,
                id,
                decision,
            } => Response::Waiting(runtime.answer_approval(&thread_id, id, decision).await),
            ThreadRequest::SetApprovals { thread_id, policy } => {
                runtime
                    .state_of(&thread_id)
                    .await
                    .set_thread_approvals(&thread_id, policy)
                    .await?;
                Response::Done
            }
            ThreadRequest::GetApprovals { thread_id } => {
                let state = runtime.state_of(&thread_id).await;
                Response::Approvals(state.thread_approvals(&thread_id).await?)
            }
            ThreadRequest::Pin {
                thread_id,
                message_id,
                pinned,
            } => {
                let pins = runtime.set_pinned(&thread_id, &message_id, pinned).await?;
                // Every client showing the chat redraws its bar.
                publish(
                    &self.subscribers,
                    Event::Thread {
                        thread_id,
                        event: Box::new(ThreadEvent::Pins(pins.clone())),
                    },
                );
                Response::Pins(pins)
            }
            ThreadRequest::SkillChanges { thread_id } => {
                Response::SkillChanges(runtime.skill_changes(&thread_id).await?)
            }
            ThreadRequest::Pins { thread_id } => Response::Pins(runtime.pins(&thread_id).await?),
            ThreadRequest::Seen { thread_id } => {
                unread::set(&runtime, &self.subscribers, &thread_id, false).await;
                Response::Done
            }
        })
    }

    /// Keeps the thread's input and forwards its events until its task ends.
    /// Opens a new chat with its own approval mode, stored before the client can send
    /// to it, so its first turn already runs under that mode.
    async fn open_with(
        &mut self,
        handle: ThreadHandle,
        approvals: Option<ApprovalPolicy>,
    ) -> Result<Response> {
        if approvals.is_some() {
            let state = self.runtime.state_of(&handle.thread_id).await;
            state
                .set_thread_approvals(&handle.thread_id, approvals)
                .await?;
        }
        Ok(self.open(handle))
    }

    pub(super) fn open(&mut self, handle: ThreadHandle) -> Response {
        let ThreadHandle {
            thread_id,
            input,
            mut output,
        } = handle;
        let subscribers = Arc::clone(&self.subscribers);
        let id = thread_id.clone();
        let done = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&done);
        let running: Arc<Mutex<Option<Running>>> = Arc::new(Mutex::new(None));
        let current = Arc::clone(&running);
        let suggested: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
        let suggestion = Arc::clone(&suggested);
        let runtime = Arc::clone(&self.runtime);
        let (tracker, syncs) = (self.pulls.clone(), self.chat_pulls.clone());
        let forward = tokio::spawn(async move {
            // A thread may say it is idle as it opens; it is done only once it has worked.
            let mut worked = false;
            while let Some(event) = output.next_event().await {
                if matches!(event, ThreadEvent::Idle) {
                    flag.store(worked, Ordering::Relaxed);
                } else {
                    worked = true;
                    flag.store(false, Ordering::Relaxed);
                }
                unread::heard(&runtime, &subscribers, &id, &event).await;
                // A message of the user or an answer may link a pull request.
                if matches!(
                    event,
                    ThreadEvent::UserMessage(_) | ThreadEvent::TurnFinished(_)
                ) {
                    let (runtime, tracker) = (Arc::clone(&runtime), tracker.clone());
                    let (subscribers, id) = (Arc::clone(&subscribers), id.clone());
                    let syncs = syncs.clone();
                    tokio::spawn(async move {
                        thread_pulls::chat_spoke(&runtime, &tracker, &subscribers, &syncs, &id)
                            .await;
                    });
                }
                // The start, the waits and the end of a turn are kept and sent under the
                // lock a joining client takes, so it hears of a running turn before its
                // end. A question is kept while the turn waits for the user's decision:
                // one the model asks is read back from the history instead.
                let turn = match &event {
                    ThreadEvent::TurnStarted(turn)
                    | ThreadEvent::TurnRunning(turn)
                    | ThreadEvent::TurnWaiting(turn) => Some(Some(Running {
                        turn: turn.clone(),
                        question: None,
                    })),
                    ThreadEvent::Question(question) => {
                        let running = current.lock().clone();
                        running
                            .filter(|running| {
                                matches!(running.turn.waiting, Some(TurnWait::Decision { .. }))
                            })
                            .map(|running| {
                                Some(Running {
                                    question: Some(question.clone()),
                                    ..running
                                })
                            })
                    }
                    // A joining client is told the totals so far with the turn.
                    ThreadEvent::TurnProgress {
                        turn_id,
                        steps,
                        tokens,
                        cost_micro_usd,
                    } => {
                        let running = current.lock().clone();
                        running
                            .filter(|running| running.turn.turn_id == *turn_id)
                            .map(|mut running| {
                                running.turn.steps = *steps;
                                running.turn.tokens = *tokens;
                                running.turn.cost_micro_usd = *cost_micro_usd;
                                Some(running)
                            })
                    }
                    ThreadEvent::TurnFinished(_) | ThreadEvent::Failed(_) | ThreadEvent::Idle => {
                        Some(None)
                    }
                    _ => None,
                };
                // Kept under the lock a joining client takes, as the turn is.
                let _suggested = suggestion_after(&event).map(|text| {
                    let mut kept = suggestion.lock();
                    *kept = text;
                    kept
                });
                let event = Event::Thread {
                    thread_id: id.clone(),
                    event: Box::new(event),
                };
                match turn {
                    Some(turn) => {
                        let mut running = current.lock();
                        *running = turn;
                        publish(&subscribers, event);
                    }
                    None => publish(&subscribers, event),
                }
            }
            // The thread is over: a client that joins while the task is finishing must
            // hear of no turn after the end.
            {
                let mut running = current.lock();
                *running = None;
                publish(&subscribers, Event::Ended { thread_id: id });
            }
        });
        let clients = HashSet::from([self.caller]);
        self.threads.insert(
            thread_id.clone(),
            OpenThread {
                input,
                forward,
                clients,
                orphaned: None,
                unattended: false,
                done,
                running,
                suggested,
            },
        );
        Response::Opened { thread_id }
    }

    /// Where the tools of a thread that is picked up again work. A client bound to chats
    /// does not choose: the agent goes on in the folder stored for the chat, and a chat
    /// without one is not opened in a folder such a client names.
    pub(super) async fn resume_folder(&self, thread_id: &str, cwd: PathBuf) -> Result<PathBuf> {
        if self.bound().is_none() {
            return Ok(cwd);
        }
        let state = self.runtime.state_of(thread_id).await;
        // Whatever the reason, the client learns only that it cannot be opened.
        let stored = state.load_thread(thread_id).await.ok().flatten();
        stored
            .and_then(|thread| thread.project)
            .map(PathBuf::from)
            .ok_or_else(|| Shown::new(NO_FOLDER).into())
    }

    pub(super) fn open_thread(&self, thread_id: &str) -> Result<&OpenThread> {
        self.threads
            .get(thread_id)
            .with_context(|| format!("thread {thread_id} is not open"))
    }

    /// Stops the thread's turn and waits for its task to end, so it can be opened again.
    pub(super) async fn close(&mut self, thread_id: &str) {
        let Some(OpenThread {
            input, mut forward, ..
        }) = self.threads.remove(thread_id)
        else {
            return;
        };
        if forward.is_finished() {
            return;
        }
        // Cancelling a thread that already ended has nothing to stop.
        let _ = input.cancel().await;
        drop(input);
        // A task that does not finish is cut, so it cannot go on beside a new one.
        if tokio::time::timeout(CLOSE_TIMEOUT, &mut forward)
            .await
            .is_err()
        {
            forward.abort();
        }
    }
}

impl Server {
    /// The caller stops using the thread; it closes when no other client keeps it open.
    async fn leave(&mut self, thread_id: &str) {
        let caller = self.caller;
        self.release(thread_id, caller).await;
    }

    /// A client went away: it leaves every thread, and the ones nobody keeps close.
    pub(super) async fn detach(&mut self, client: ClientId) {
        self.clients.remove(&client);
        if self.owner == Some(client) {
            self.owner = None;
        }
        self.pulls.forget(client);
        self.user_runs.stop_client(client);
        self.subscribers
            .lock()
            .retain(|subscriber| subscriber.client != client);
        // A shell never outlives the client that opened it.
        self.terminals.close_client(client);
        // Nor does a part of a plugin's window.
        self.runtime.plugin_ui().close_client(client);
        let ids: Vec<String> = self.threads.keys().cloned().collect();
        // A client that is gone, unlike one that closes a thread, may be a browser that
        // reloads: its threads run on for a while before they are stopped.
        for thread_id in ids {
            if let Some(open) = self.threads.get_mut(&thread_id)
                && open.clients.remove(&client)
                && open.clients.is_empty()
                && !open.unattended
            {
                open.orphaned = Some(std::time::Instant::now());
            }
        }
    }

    /// A client that asked to close the thread: it ends now if nobody else keeps it.
    async fn release(&mut self, thread_id: &str, client: ClientId) {
        let Some(open) = self.threads.get_mut(thread_id) else {
            return;
        };
        open.clients.remove(&client);
        // A scheduled run someone looked at goes on to the end of its turn.
        let running_alone = open.unattended && !open.done.load(Ordering::Relaxed);
        if open.clients.is_empty() && !running_alone {
            self.close(thread_id).await;
        }
    }

    /// Stops the threads nobody has kept open for longer than the grace.
    pub(super) async fn close_orphans(&mut self) {
        let stale: Vec<String> = self
            .threads
            .iter()
            .filter(|(_, open)| {
                let finished =
                    open.unattended && open.clients.is_empty() && open.done.load(Ordering::Relaxed);
                finished
                    || open
                        .orphaned
                        .is_some_and(|at| at.elapsed() > super::ORPHAN_GRACE)
            })
            .map(|(id, _)| id.clone())
            .collect();
        for thread_id in stale {
            self.close(&thread_id).await;
        }
    }
}
