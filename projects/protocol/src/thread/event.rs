//! What a thread tells its client: every change a client shows, as one event type.

use serde::{Deserialize, Serialize};

use crate::agent::AgentPhase;
use crate::model::{Effort, ModelEvent, ProviderErrorKind, ToolCall};
use crate::thread::{
    Compaction, Goal, LocalUnsent, Message, Pin, Task, Team, ThreadTokens, Turn, TurnFailure,
};
use crate::tools::{ApprovalRequest, Question, ToolResult};
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ThreadEvent {
    /// The active model changed or was discovered after the thread opened.
    ModelSelected {
        name: String,
        /// The largest request to it: its window or the size set under models.
        input_limit: u64,
    },
    /// Saved locally, outside the model queue, until the client explicitly resends it.
    LocalUnsent(LocalUnsent),
    /// No model is chosen, so a message waits as local unsent until one is. A provider
    /// that fails a turn says so in `TurnFailed`.
    NoModel,
    Queued {
        message_id: String,
    },
    /// A client's request joined the persisted model history, including explicit resends.
    UserMessage(Message),
    Rejected {
        reason: String,
    },
    TurnStarted(Turn),
    /// The thread is in the middle of `turn`; a client that joined after it began sets
    /// its chat to working. Unlike a replayed `TurnStarted` it opens no fold boundary:
    /// what the client read from history belongs to the same turn.
    TurnRunning(Turn),
    /// The running turn waits, as `turn.waiting` says, for the user or for the provider;
    /// it is still the running turn. The next `TurnRunning` of it ends the wait.
    TurnWaiting(Turn),
    /// A model call failed in a way that may pass; the next attempt follows after a pause.
    /// A client keeps one row for it that each of these replaces; the first `Model` event
    /// of the call is the sign it answered (see `transcript::retry_row`).
    Retrying {
        /// The attempt that failed, from 1.
        attempt: u32,
        /// Safe to show, like `TurnFailure::error`.
        error: String,
        kind: ProviderErrorKind,
        /// The pause before the next attempt.
        wait_ms: u64,
        /// None when the call is tried until a time budget runs out instead, as for a
        /// provider that cannot be reached or does not answer in time.
        max_attempts: Option<u32>,
    },
    /// Everything the model streams: reasoning, text, tool calls, usage.
    Model(ModelEvent),
    ToolStarted(ToolCall),
    ToolFinished {
        call_id: String,
        result: ToolResult,
    },
    /// The turn ended on an error: the one row that says why. It comes right before that
    /// turn's `TurnFinished`, and again when a chat whose newest turn failed is opened.
    TurnFailed(TurnFailure),
    /// The model repeated itself and the core stepped in: the result of the call just
    /// finished carries a note, and at `RepeatRung::Stop` or `RepeatRung::JudgeStop` the
    /// turn ends. `count` is what the rung counts in this turn: the same call or error
    /// coming back, a cycle's rounds, test runs with no change between, or steps with no
    /// progress.
    RepeatNotice {
        rung: RepeatRung,
        count: u32,
    },
    TurnFinished(Turn),
    /// A model call of the running turn ended: the turn's totals so far, as `Turn` keeps
    /// them, so a client shows them without counting.
    TurnProgress {
        turn_id: String,
        steps: u32,
        tokens: u64,
        cost_micro_usd: u64,
    },
    /// The thread got a name: the start of its first message at once, then one the model
    /// gives after the first turn.
    TitleChanged {
        title: String,
    },
    /// The model reached its output cap in one call; the turn ends with what it wrote.
    OutputLimitReached,
    /// The model refused pictures; they stay files it is told the path of.
    PicturesRefused {
        model: String,
    },
    /// The model's answer broke off, such as on a dropped connection; the turn ends with
    /// what it wrote.
    AnswerCut {
        error: String,
    },
    /// The model is summarizing the context; its text streams as `Model` events.
    CompactionStarted,
    CompactionFinished(Compaction),
    /// The context stays as it was.
    CompactionFailed {
        error: String,
    },
    /// Old tool results were put away: from this request on, each is sent as a one-line
    /// stub that says how to read it back, and the whole text stays stored. Clients
    /// show nothing for it; evals count it.
    ResultsStubbed {
        results: u32,
        /// Estimated tokens each later request carries less.
        saved_tokens: u64,
    },
    /// The result of the call just finished is sent shaped: a command's output with
    /// passes, progress and repeats left out and a note on how to read it whole, which
    /// stays stored. Clients show nothing for it; evals count it.
    ResultShaped {
        /// Estimated tokens each later request carries less.
        saved_tokens: u64,
    },
    /// The core wrote down what ended a problem in this saved project
    /// (`limits.episodes`). Clients show nothing for it; evals count it.
    EpisodeRecorded {
        kind: EpisodeKind,
    },
    /// The thread's token counts, when it starts and after each turn and compaction. `limit` is the largest request: the
    /// configured input limit or the model window, whichever is smaller.
    Tokens {
        tokens: ThreadTokens,
        limit: u64,
    },
    /// The output generation rate of this thread's agent, over a short rolling window:
    /// about once a second while it generates, and once when a model call ends. A
    /// subagent's rate reaches the client wrapped in `Subagent`, whose `thread_id` says
    /// which agent it is.
    OutputRate {
        /// Generated tokens per second over `window_ms`, counted as context sizes are.
        tokens_per_sec: u64,
        /// How wide the window the average covers is, in milliseconds.
        window_ms: u64,
    },
    /// The context is past the input limit, so the queued turn was not started or the
    /// running one ended; the client should switch model or compact.
    ContextTooLarge {
        tokens: u64,
        limit: u64,
    },
    /// The tool call `call_id` gave `task` to `agent`, or the team gave a member the
    /// plan's task `task_id`; the agent works in thread `thread_id`
    /// on `model` (`provider/model`) and reasons at `effort`; `None` is the provider's
    /// default.
    SubagentStarted {
        call_id: String,
        thread_id: String,
        agent: String,
        task: String,
        effort: Option<Effort>,
        /// None when no model is chosen, and in events of older cores.
        #[serde(default)]
        model: Option<String>,
        /// The task of the plan a team member was given, its `call_id` then
        /// `team:<task_id>`; none for an agent a tool call asked.
        #[serde(default)]
        task_id: Option<String>,
    },
    /// An event of a subagent's thread. A subagent's own subagents come wrapped once more.
    Subagent {
        thread_id: String,
        event: Box<ThreadEvent>,
    },
    /// An agent below this chat, a subagent or a team member, is at work (true), or the
    /// last of them ended (false). The chat counts as working while they are, whatever its
    /// own turn does.
    AgentsWorking(bool),
    /// The thread's task board changed: every task with its steps.
    Tasks(Vec<Task>),
    /// One task of the plan this thread leads changed; `Tasks` stays for the board.
    TaskChanged(Task),
    /// The team running this thread's plan changed its status, members or budget; at
    /// most one a second.
    Team(Team),
    /// An agent's phase changed, by its own `agent_state` call or the runtime's.
    AgentPhase {
        phase: AgentPhase,
        note: String,
    },
    /// The agent asks the user to choose; the answer comes as the user's next message.
    Question(Question),
    /// A tool waits for the user's yes; the client answers through the runtime.
    Approval(ApprovalRequest),
    /// The arbiter answered a request of this thread in the user's place.
    ArbiterDecided {
        what: String,
        allow: bool,
        reason: String,
    },
    /// The request is answered or its call is gone; the client stops showing it.
    ApprovalClosed {
        id: u64,
    },
    /// Another agent wrote to this thread; the message has joined it.
    FromAgent(Message),
    /// A background process result joined this thread at a safe execution boundary.
    FromJob(Message),
    /// The standing goal changed, or the thread opened with one; `note` says what
    /// happened in a line.
    Goal {
        goal: Option<Goal>,
        note: String,
        /// The message queued to start work on it, for a client to show as sent.
        started: Option<String>,
    },
    /// /undo took the thread back; `text` is the message it went back to, for the
    /// client to offer again.
    Undone {
        text: String,
    },
    /// The thread's pins changed, by a pin, an unpin or an /undo that took a pinned
    /// message: all of them, oldest message first.
    Pins(Vec<Pin>),
    /// Something for the user arrived in the chat (true), or a client showed it to them
    /// (false): its `Thread::unread` changed. Every client listing the chat hears it.
    Unread(bool),
    /// The chat's pull requests changed: one was linked, or the forge told a new state,
    /// title or size. All of them, as `Thread::pulls`. Every client listing the chat
    /// hears it.
    Pulls(Vec<crate::project::ThreadPull>),
    /// The thread branched off into a worktree (`Some`), or merged or abandoned it and
    /// works in its own folder again (`None`): its `Thread::branch` changed.
    Branch(Option<crate::project::ThreadBranch>),
    /// A skill file changed by a tool call of this thread, or such a change was undone.
    SkillChanged(crate::catalog::SkillChange),
    /// /fork wrote its summary; a client opens a new thread that starts from it.
    Forked {
        summary: String,
    },
    /// The model proposed the user's next message (`suggest_next`); a composer shows it
    /// greyed in its empty input until the user says something or a new turn starts
    /// (`ThreadEvent::ends_suggestion`). Only a chat's own: a subagent has no user.
    NextSuggested {
        text: String,
    },
    /// Nothing is running and nothing will start until the next message.
    Idle,
    /// The thread stopped on an error it could not handle, such as a database failure.
    /// A turn that fails says so in `TurnFailed` instead.
    Failed(String),
}

/// How far the core went when the model repeated itself within a turn.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RepeatRung {
    /// The same call with the same result again: the model is told to change course.
    SameCall,
    /// The same error again, through calls that differ: the model is told to stop
    /// varying the call and check its assumption.
    SameError,
    /// The same error yet again: the next step runs at the effort set and the model is
    /// asked for a different plan.
    Replan,
    /// The turn ends as failed, with the error and the attempts.
    Stop,
    /// The same short run of calls with the same results came round again, or one call
    /// gave the same result several times with nothing changed between: the model is told
    /// it is going in circles.
    Cycle,
    /// The cycle went on: the model is asked for a different plan.
    CycleReplan,
    /// The tests ran again with no project file changed since their last run: the model
    /// is told what that run found.
    TestRerun,
    /// The tests kept running with nothing changed: the model is asked for a different
    /// plan.
    TestRerunReplan,
    /// Steps went by with nothing new: no file changed, nothing new read, nothing that
    /// failed passed. The model is asked where it stands.
    NoProgress,
    /// Still no progress: the model is asked for a different plan.
    NoProgressReplan,
    /// A judge model read the turn after a rule fired and found it not stuck; its
    /// advice goes to the model and the turn goes on.
    JudgeRedirect,
    /// A judge model confirmed the turn is stuck: the turn ends as failed, with why.
    JudgeStop,
}

impl RepeatRung {
    /// Whether the turn ends at this rung.
    pub fn stops(self) -> bool {
        matches!(self, Self::Stop | Self::JudgeStop)
    }
}

/// What an episode the core wrote down is about.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EpisodeKind {
    /// An error that came back ended with a call that passed.
    Fixed,
    /// A goal's verification passed.
    Verified,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::{ApprovalRequest, Request};

    #[test]
    fn events_survive_a_round_trip() {
        let event = ThreadEvent::Subagent {
            thread_id: "child".into(),
            event: Box::new(ThreadEvent::Approval(ApprovalRequest {
                id: 7,
                request: Request::Command {
                    command: "cargo test".into(),
                },
                session: Some("cargo".into()),
                steps: Vec::new(),
            })),
        };
        let text = serde_json::to_string(&event).unwrap();
        assert_eq!(serde_json::from_str::<ThreadEvent>(&text).unwrap(), event);
    }
}
