//! What a thread is doing right now, for the one indicator every client shows while it
//! works. `LiveTurn` keeps the few signals the events carry; `activity` turns them into a
//! kind and its words. The clients draw the kinds each in their own way, and none of them
//! chooses the kind itself.

use crate::model::ModelEvent;
use crate::thread::{Retry, RetryRow, ThreadEvent, TurnWait, retry_row};
use crate::tools::{is_agent_tool, phrase};

/// After this long without a word from the core, a thinking turn counts as stalled.
pub const STALLED_AFTER_MS: i64 = 30_000;

/// The tool that writes the plan.
const PLAN_TOOL: &str = "tasks";

/// The kinds of work a client tells apart. Each has a look of its own; `Writing` and
/// `WaitingForUser` are drawn without motion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkKind {
    /// Before anything shows, while the model reasons, and between steps.
    Thinking,
    /// The answer's text is streaming: the text arriving is the signal.
    Writing,
    /// The plan is being written or changed.
    Planning,
    /// Tools run: the step list is the indicator.
    Steps,
    /// Agents below the thread work while it waits for them. `started` counts the agents
    /// of this burst, those done among them.
    Agents { working: u32, started: u32 },
    /// An approval, a question or a decision is open.
    WaitingForUser,
    /// The context is being summarized.
    Compacting,
    /// The provider makes the turn wait, or a model call is tried again.
    Held,
}

/// What the indicator shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Work {
    pub kind: WorkKind,
    /// The words beside the mark, such as "Thinking" or "Running a command".
    pub label: String,
    /// Nothing came for `STALLED_AFTER_MS` while the model was to speak.
    pub stalled: bool,
}

/// What the model streamed last, since the last step.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum Stream {
    #[default]
    Nothing,
    Reasoning,
    Text,
    /// A tool call is being written; true when it is the plan's.
    Call(bool),
}

/// The live signals of one thread. Feed it every event of the thread with `apply`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LiveTurn {
    running: bool,
    waiting: Option<TurnWait>,
    compacting: bool,
    /// The model call being tried again and when its event came.
    retry: Option<(Retry, i64)>,
    /// The tools that run, by call and name, oldest first.
    tools: Vec<(String, String)>,
    stream: Stream,
    approvals: Vec<u64>,
    questions: u32,
    /// The agents of this burst by thread, and whether each is at work.
    agents: Vec<(String, bool)>,
    agents_on: bool,
    /// When the last event came.
    heard: i64,
}

impl LiveTurn {
    /// Request `id` was answered here; the core's close may come later or not at all.
    pub fn close_approval(&mut self, id: u64) {
        self.approvals.retain(|open| *open != id);
    }

    /// Takes the open requests `later`, followed live, knows of.
    pub fn merge_approvals(&mut self, later: &LiveTurn) {
        for id in &later.approvals {
            if !self.approvals.contains(id) {
                self.approvals.push(*id);
            }
        }
    }

    /// Takes in `event`, which came at `now` (Unix milliseconds, the client's clock).
    pub fn apply(&mut self, event: &ThreadEvent, now: i64) {
        self.heard = now;
        match retry_row(event) {
            Some(RetryRow::Show(retry)) => self.retry = Some((retry, now)),
            Some(RetryRow::Clear) => self.retry = None,
            None => {}
        }
        // An agent's request at any depth waits on the user as the chat's own does.
        match event.deepest().1 {
            ThreadEvent::Approval(request) if !self.approvals.contains(&request.id) => {
                self.approvals.push(request.id)
            }
            ThreadEvent::ApprovalClosed { id } => self.approvals.retain(|open| open != id),
            _ => {}
        }
        match event {
            ThreadEvent::TurnStarted(turn) => {
                self.running = true;
                self.waiting = turn.waiting;
                self.tools.clear();
                self.stream = Stream::Nothing;
                self.questions = 0;
            }
            ThreadEvent::TurnRunning(turn) | ThreadEvent::TurnWaiting(turn) => {
                self.running = true;
                self.waiting = turn.waiting;
            }
            ThreadEvent::TurnFinished(_) | ThreadEvent::Idle | ThreadEvent::Failed(_) => {
                self.running = false;
                self.waiting = None;
                self.compacting = false;
                self.retry = None;
                self.tools.clear();
                self.stream = Stream::Nothing;
            }
            ThreadEvent::Model(ModelEvent::Reasoning(_)) => self.stream = Stream::Reasoning,
            ThreadEvent::Model(ModelEvent::Text(_)) => self.stream = Stream::Text,
            ThreadEvent::Model(ModelEvent::ToolCall { name, .. }) => {
                self.stream = Stream::Call(name == PLAN_TOOL);
            }
            ThreadEvent::ToolStarted(call) => {
                self.tools.push((call.id.clone(), call.name.clone()));
                self.stream = Stream::Nothing;
            }
            ThreadEvent::ToolFinished { call_id, .. } => {
                self.tools.retain(|(id, _)| id != call_id);
                self.stream = Stream::Nothing;
            }
            ThreadEvent::CompactionStarted => self.compacting = true,
            ThreadEvent::CompactionFinished(_) | ThreadEvent::CompactionFailed { .. } => {
                self.compacting = false;
            }
            ThreadEvent::Question(_) => self.questions += 1,
            ThreadEvent::UserMessage(_) => self.questions = 0,
            ThreadEvent::SubagentStarted { thread_id, .. } => {
                self.agents.retain(|(id, _)| id != thread_id);
                self.agents.push((thread_id.clone(), true));
            }
            ThreadEvent::Subagent { thread_id, event } => {
                let working = match **event {
                    ThreadEvent::TurnStarted(_) => true,
                    ThreadEvent::Idle | ThreadEvent::Failed(_) | ThreadEvent::TurnFailed(_) => {
                        false
                    }
                    _ => return,
                };
                if let Some(agent) = self.agents.iter_mut().find(|(id, _)| id == thread_id) {
                    agent.1 = working;
                }
            }
            ThreadEvent::AgentsWorking(on) => {
                self.agents_on = *on;
                // The burst is over: the next agent starts a new count.
                if !on {
                    self.agents.clear();
                }
            }
            _ => {}
        }
    }

    /// The agent of `thread_id` stopped without an event of its own, as a team's end
    /// stops a member that never ran: the working row stops counting it.
    pub fn agent_stopped(&mut self, thread_id: &str) {
        if let Some(agent) = self.agents.iter_mut().find(|(id, _)| id == thread_id) {
            agent.1 = false;
        }
    }

    /// The user answered every question on the client's side; the message is on its way.
    pub fn answered(&mut self) {
        self.questions = 0;
    }

    /// A model call is being tried again.
    pub fn retrying(&self) -> bool {
        self.retry.is_some()
    }

    /// The turn of this thread runs, whatever the agents below it do.
    pub fn running(&self) -> bool {
        self.running
    }

    fn agents(&self) -> Option<(u32, u32)> {
        let working = self.agents.iter().filter(|(_, working)| *working).count() as u32;
        let started = self.agents.len() as u32;
        if working > 0 {
            Some((working, started))
        } else {
            // The core says agents work that this client never saw start.
            self.agents_on.then_some((1, started.max(1)))
        }
    }
}

/// What the thread does at `now` (Unix milliseconds), or none when nothing runs: then no
/// indicator shows and nothing moves.
pub fn activity(live: &LiveTurn, now: i64) -> Option<Work> {
    let agents = live.agents();
    // A call tried again is work, whether or not this client saw its turn start.
    if !live.running && !live.compacting && live.retry.is_none() && agents.is_none() {
        return None;
    }
    let work = |kind, label: &str| {
        Some(Work {
            kind,
            label: label.to_owned(),
            stalled: false,
        })
    };
    // An agent's request waits while the chat's own turn has ended, as a team's does.
    if !live.approvals.is_empty() {
        return work(WorkKind::WaitingForUser, "Waiting for your approval");
    }
    if live.running && live.questions > 0 {
        return work(WorkKind::WaitingForUser, "Waiting for your answer");
    }
    match live.waiting {
        Some(wait @ TurnWait::Decision { .. }) => {
            return work(WorkKind::WaitingForUser, &wait.text(None));
        }
        Some(wait @ TurnWait::Provider { .. }) => {
            return work(WorkKind::Held, &wait.text(Some(now)));
        }
        None => {}
    }
    if let Some((retry, arrived)) = &live.retry {
        let waited = u64::try_from(now.saturating_sub(*arrived)).unwrap_or(0);
        return work(WorkKind::Held, &retry.line(waited));
    }
    if live.compacting {
        return work(WorkKind::Compacting, "Compacting the context");
    }
    let agents_kind = |(working, started): (u32, u32)| {
        let label = match working {
            1 => "1 agent working".to_owned(),
            count => format!("{count} agents working"),
        };
        work(WorkKind::Agents { working, started }, &label)
    };
    if !live.running {
        return agents.and_then(agents_kind);
    }
    if let Some((id, name)) = live.tools.last() {
        // The thread only waits for its agents: they are the work.
        if let Some(agents) = agents
            && live.tools.iter().all(|(_, name)| is_agent_tool(name))
        {
            return agents_kind(agents);
        }
        if name == PLAN_TOOL {
            return work(WorkKind::Planning, "Updating the plan");
        }
        return work(WorkKind::Steps, phrase(name, id));
    }
    match live.stream {
        Stream::Text => work(WorkKind::Writing, "Writing"),
        Stream::Call(true) => work(WorkKind::Planning, "Updating the plan"),
        Stream::Nothing | Stream::Reasoning | Stream::Call(false) => Some(Work {
            kind: WorkKind::Thinking,
            label: "Thinking".to_owned(),
            stalled: now.saturating_sub(live.heard) >= STALLED_AFTER_MS,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ProviderErrorKind, ToolCall};
    use crate::thread::{Turn, TurnStatus};
    use crate::tools::{ApprovalRequest, Question, Request, ToolResult};

    fn turn(waiting: Option<TurnWait>) -> Turn {
        Turn {
            turn_id: "turn".into(),
            thread_id: "thread".into(),
            last_sequence: 1,
            response_message_id: None,
            status: TurnStatus::Running,
            started_at: 0,
            finished_at: None,
            error_message: None,
            waiting,
            steps: 0,
            tokens: 0,
            cost_micro_usd: 0,
        }
    }

    fn started(id: &str, name: &str) -> ThreadEvent {
        ThreadEvent::ToolStarted(ToolCall {
            id: id.into(),
            name: name.into(),
            arguments: serde_json::json!({}),
        })
    }

    fn finished(id: &str) -> ThreadEvent {
        ThreadEvent::ToolFinished {
            call_id: id.into(),
            result: ToolResult::text("ok".into()),
        }
    }

    fn text() -> ThreadEvent {
        ThreadEvent::Model(ModelEvent::Text("word".into()))
    }

    fn agent(thread: &str) -> ThreadEvent {
        ThreadEvent::SubagentStarted {
            call_id: format!("call-{thread}"),
            thread_id: thread.into(),
            agent: "general".into(),
            task: "look".into(),
            effort: None,
            model: None,
            task_id: None,
        }
    }

    fn of_agent(thread: &str, event: ThreadEvent) -> ThreadEvent {
        ThreadEvent::Subagent {
            thread_id: thread.into(),
            event: Box::new(event),
        }
    }

    /// A turn under way, fed `events` one after another at time zero.
    fn live(events: &[ThreadEvent]) -> LiveTurn {
        let mut live = LiveTurn::default();
        live.apply(&ThreadEvent::TurnStarted(turn(None)), 0);
        for event in events {
            live.apply(event, 0);
        }
        live
    }

    fn kind(live: &LiveTurn) -> Option<WorkKind> {
        activity(live, 0).map(|work| work.kind)
    }

    #[test]
    fn nothing_shows_before_a_turn_and_after_it() {
        let mut live = LiveTurn::default();
        assert_eq!(activity(&live, 0), None);
        live.apply(&ThreadEvent::TurnStarted(turn(None)), 0);
        assert!(live.running());
        live.apply(&started("a", "read_file"), 0);
        for end in [
            ThreadEvent::Idle,
            ThreadEvent::TurnFinished(turn(None)),
            ThreadEvent::Failed("gone".into()),
        ] {
            let mut ended = live.clone();
            ended.apply(&end, 0);
            assert_eq!(activity(&ended, 0), None, "{end:?}");
            assert!(!ended.running());
        }
    }

    #[test]
    fn a_turn_thinks_until_something_shows_and_while_it_reasons() {
        let fresh = live(&[]);
        assert_eq!(
            activity(&fresh, 0),
            Some(Work {
                kind: WorkKind::Thinking,
                label: "Thinking".into(),
                stalled: false,
            })
        );
        let reasoning = live(&[ThreadEvent::Model(ModelEvent::Reasoning("hm".into()))]);
        assert_eq!(kind(&reasoning), Some(WorkKind::Thinking));
        // A client that joined a turn under way knows no more than that it runs.
        let mut joined = LiveTurn::default();
        joined.apply(&ThreadEvent::TurnRunning(turn(None)), 0);
        assert_eq!(kind(&joined), Some(WorkKind::Thinking));
    }

    #[test]
    fn thinking_stalls_after_a_long_silence_and_wakes_with_the_next_event() {
        let mut live = live(&[]);
        assert!(!activity(&live, STALLED_AFTER_MS - 1).unwrap().stalled);
        assert!(activity(&live, STALLED_AFTER_MS).unwrap().stalled);
        live.apply(
            &ThreadEvent::Model(ModelEvent::Reasoning("so".into())),
            40_000,
        );
        assert!(!activity(&live, 41_000).unwrap().stalled);
        // A command may run long: that is not a stall.
        live.apply(&started("a", "run_command"), 41_000);
        let running = activity(&live, 400_000).unwrap();
        assert_eq!((running.kind, running.stalled), (WorkKind::Steps, false));
    }

    #[test]
    fn streaming_text_is_writing() {
        let writing = live(&[text()]);
        assert_eq!(
            activity(&writing, 0),
            Some(Work {
                kind: WorkKind::Writing,
                label: "Writing".into(),
                stalled: false,
            })
        );
        // Silence while writing is not flagged: the text itself shows it stopped.
        assert!(!activity(&writing, 90_000).unwrap().stalled);
    }

    #[test]
    fn a_running_tool_is_a_step_named_as_a_person_would() {
        let step = live(&[started("a", "run_command")]);
        let work = activity(&step, 0).unwrap();
        assert_eq!(work.kind, WorkKind::Steps);
        assert_eq!(work.label, phrase("run_command", "a"));
        // Of several at once, the newest names the row; the others still count.
        let mut two = live(&[started("a", "run_command"), started("b", "read_file")]);
        assert_eq!(activity(&two, 0).unwrap().label, phrase("read_file", "b"));
        two.apply(&finished("b"), 0);
        assert_eq!(activity(&two, 0).unwrap().label, phrase("run_command", "a"));
    }

    #[test]
    fn the_plan_is_planning_from_its_first_streamed_piece() {
        let drafting = live(&[ThreadEvent::Model(ModelEvent::ToolCall {
            index: 0,
            id: "p".into(),
            name: "tasks".into(),
        })]);
        assert_eq!(
            activity(&drafting, 0),
            Some(Work {
                kind: WorkKind::Planning,
                label: "Updating the plan".into(),
                stalled: false,
            })
        );
        let running = live(&[started("p", "tasks")]);
        assert_eq!(kind(&running), Some(WorkKind::Planning));
        // Another tool's call being written is still thinking.
        let other = live(&[
            text(),
            ThreadEvent::Model(ModelEvent::ToolCall {
                index: 0,
                id: "r".into(),
                name: "read_file".into(),
            }),
        ]);
        assert_eq!(kind(&other), Some(WorkKind::Thinking));
    }

    #[test]
    fn agents_are_counted_while_the_thread_only_waits_for_them() {
        let mut live = live(&[
            started("call-a", "ask_agent"),
            agent("a"),
            started("call-b", "ask_agent"),
            agent("b"),
            ThreadEvent::AgentsWorking(true),
        ]);
        assert_eq!(
            activity(&live, 0),
            Some(Work {
                kind: WorkKind::Agents {
                    working: 2,
                    started: 2
                },
                label: "2 agents working".into(),
                stalled: false,
            })
        );
        live.apply(&of_agent("a", ThreadEvent::Idle), 0);
        let one = activity(&live, 0).unwrap();
        assert_eq!(
            (one.kind, one.label.as_str()),
            (
                WorkKind::Agents {
                    working: 1,
                    started: 2
                },
                "1 agent working"
            )
        );
        // The thread does something of its own beside them: that shows.
        live.apply(&started("c", "read_file"), 0);
        assert_eq!(kind(&live), Some(WorkKind::Steps));
        live.apply(&finished("c"), 0);
        // Its turn ends and the agent works on: the indicator stays for the agent.
        live.apply(&ThreadEvent::Idle, 0);
        assert_eq!(
            kind(&live),
            Some(WorkKind::Agents {
                working: 1,
                started: 2
            })
        );
        live.apply(&ThreadEvent::AgentsWorking(false), 0);
        assert_eq!(activity(&live, 0), None);
        // The next burst counts from one.
        live.apply(&agent("c"), 0);
        assert_eq!(
            kind(&live),
            Some(WorkKind::Agents {
                working: 1,
                started: 1
            })
        );
    }

    #[test]
    fn agents_the_client_never_saw_start_still_show() {
        let mut live = LiveTurn::default();
        live.apply(&ThreadEvent::AgentsWorking(true), 0);
        assert_eq!(activity(&live, 0).unwrap().label, "1 agent working");
    }

    #[test]
    fn whatever_waits_for_the_user_comes_first_and_stands_still() {
        let approval = ApprovalRequest {
            id: 7,
            request: Request::Command {
                command: "cargo publish".into(),
            },
            session: None,
            steps: Vec::new(),
        };
        let mut live = live(&[started("a", "run_command"), ThreadEvent::Approval(approval)]);
        let work = activity(&live, 0).unwrap();
        assert_eq!(
            (work.kind, work.label.as_str()),
            (WorkKind::WaitingForUser, "Waiting for your approval")
        );
        live.apply(&ThreadEvent::ApprovalClosed { id: 7 }, 0);
        assert_eq!(kind(&live), Some(WorkKind::Steps));

        live.apply(
            &ThreadEvent::Question(Question {
                question: "Which?".into(),
                options: vec!["a".into(), "b".into()],
                multiple: false,
            }),
            0,
        );
        assert_eq!(activity(&live, 0).unwrap().label, "Waiting for your answer");
        live.answered();
        assert_eq!(kind(&live), Some(WorkKind::Steps));

        let decision = TurnWait::Decision { since: 0 };
        live.apply(&ThreadEvent::TurnWaiting(turn(Some(decision))), 0);
        let work = activity(&live, 0).unwrap();
        assert_eq!(
            (work.kind, work.label.as_str()),
            (WorkKind::WaitingForUser, "Waiting for your decision")
        );
        live.apply(&ThreadEvent::TurnRunning(turn(None)), 0);
        assert_eq!(kind(&live), Some(WorkKind::Steps));
    }

    #[test]
    fn compacting_has_its_own_kind_with_or_without_a_turn() {
        let mut alone = LiveTurn::default();
        alone.apply(&ThreadEvent::CompactionStarted, 0);
        assert_eq!(
            activity(&alone, 0),
            Some(Work {
                kind: WorkKind::Compacting,
                label: "Compacting the context".into(),
                stalled: false,
            })
        );
        // The summary streams as text; it is still compacting.
        alone.apply(&text(), 0);
        assert_eq!(kind(&alone), Some(WorkKind::Compacting));
        alone.apply(&ThreadEvent::CompactionFailed { error: "no".into() }, 0);
        assert_eq!(activity(&alone, 0), None);

        let mut within = live(&[ThreadEvent::CompactionStarted]);
        assert_eq!(kind(&within), Some(WorkKind::Compacting));
        within.apply(&ThreadEvent::Idle, 0);
        assert_eq!(activity(&within, 0), None);
    }

    #[test]
    fn a_provider_wait_and_a_retry_hold_the_turn_with_their_countdown() {
        let wait = TurnWait::provider(ProviderErrorKind::RateLimited, 0, 60_000).unwrap();
        let mut live = live(&[ThreadEvent::TurnWaiting(turn(Some(wait)))]);
        let work = activity(&live, 48_000).unwrap();
        assert_eq!(work.kind, WorkKind::Held);
        assert_eq!(work.label, wait.text(Some(48_000)));
        live.apply(&ThreadEvent::TurnRunning(turn(None)), 60_000);
        assert_eq!(kind(&live), Some(WorkKind::Thinking));

        live.apply(
            &ThreadEvent::Retrying {
                attempt: 1,
                error: "timeout".into(),
                kind: ProviderErrorKind::Overloaded,
                wait_ms: 8_000,
                max_attempts: Some(3),
            },
            60_000,
        );
        let work = activity(&live, 63_000).unwrap();
        assert_eq!(work.kind, WorkKind::Held);
        assert!(work.label.ends_with("next in 5 s"), "{}", work.label);
        // The call answers: the first model event ends the hold.
        live.apply(&text(), 70_000);
        assert_eq!(kind(&live), Some(WorkKind::Writing));

        // A client that never saw the turn start still shows the retry.
        let mut joined = LiveTurn::default();
        joined.apply(
            &ThreadEvent::Retrying {
                attempt: 2,
                error: "timeout".into(),
                kind: ProviderErrorKind::Overloaded,
                wait_ms: 1_000,
                max_attempts: None,
            },
            0,
        );
        assert_eq!(kind(&joined), Some(WorkKind::Held));
    }

    #[test]
    fn the_kind_follows_the_turn_from_step_to_text_and_back() {
        let mut live = live(&[]);
        let mut seen = vec![kind(&live).unwrap()];
        for event in [
            ThreadEvent::Model(ModelEvent::Reasoning("plan".into())),
            started("a", "read_file"),
            finished("a"),
            text(),
            started("b", "tasks"),
            finished("b"),
            text(),
        ] {
            live.apply(&event, 0);
            seen.push(kind(&live).unwrap());
        }
        use WorkKind::*;
        assert_eq!(
            seen,
            [
                Thinking, Thinking, Steps, Thinking, Writing, Planning, Thinking, Writing
            ]
        );
        // A tool finished while text resumes: writing, whatever ran before.
        live.apply(&started("c", "run_command"), 0);
        live.apply(&finished("c"), 0);
        live.apply(&text(), 0);
        assert_eq!(kind(&live), Some(Writing));
    }

    #[test]
    fn a_new_turn_forgets_the_steps_and_questions_of_the_last() {
        let mut live = live(&[
            started("a", "run_command"),
            ThreadEvent::Question(Question {
                question: "Which?".into(),
                options: vec!["a".into(), "b".into()],
                multiple: false,
            }),
        ]);
        live.apply(&ThreadEvent::TurnStarted(turn(None)), 0);
        assert_eq!(kind(&live), Some(WorkKind::Thinking));
    }
}
