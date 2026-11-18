//! A turn a stopped process left running goes on where it stopped, and nothing that may
//! have taken effect runs twice. Only the first open call of the last step can have
//! started, or, when it is read-only, the reads after it that ran with it (they all may
//! run again): when it may simply run again, the turn goes on by itself; when it may not, a
//! chat asks the user and an agent, with no one to ask, stops. A turn left long ago asks
//! too, so opening an old chat spends no tokens unasked.

use super::{Agent, StepEnd};
use crate::providers::ToolCall;
use crate::runtime::ThreadEvent;
use crate::runtime::agent::{Inbox, Stop};
use crate::state::{
    AgentPhase, CHECK_NOTE, CUT_OFF, PendingMessage, Settle, Turn, TurnOutcome, TurnStatus,
    Unfinished,
};
use anyhow::{Result, ensure};
use dusk_protocol::thread::{TurnWait, duration_words};
use dusk_protocol::tools::Question;
use tracing::info;

/// What a restart found of the thread's running turn.
#[derive(Debug, Clone, PartialEq)]
pub enum Recovered {
    /// No turn was left running, or it was stopped.
    Nothing,
    /// The turn goes on by itself.
    Resume(Turn),
    /// The turn waits for the user's answer to the question.
    Decide(Turn, Question),
}

/// The user's answer to the question a restart asked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Continue,
    CheckFirst,
    Stop,
}

const CONTINUE: &str = "Continue";
const CHECK_FIRST: &str = "Check first";
const STOP: &str = "Stop";

/// How much of the cut-off call's arguments the question shows.
const SHOWN_ARGUMENTS: usize = 80;

impl Decision {
    /// The decision `content` gives when it is the answer to `question` with one of its
    /// options, as `Question::reply` writes it; `None` for any other message.
    pub fn of(question: &Question, content: &str) -> Option<Self> {
        let option = question
            .options
            .iter()
            .find(|option| content == question.reply(option))?;
        match option.as_str() {
            CONTINUE => Some(Self::Continue),
            CHECK_FIRST => Some(Self::CheckFirst),
            STOP => Some(Self::Stop),
            _ => None,
        }
    }
}

/// The question about a call a restart cut off.
pub fn cut_off_question(call: &ToolCall) -> Question {
    let arguments = one_line(&call.arguments.to_string(), SHOWN_ARGUMENTS);
    Question {
        question: format!("A restart cut off `{}`: {arguments}. {CUT_OFF}", call.name),
        options: [CONTINUE, CHECK_FIRST, STOP].map(String::from).to_vec(),
        multiple: false,
    }
}

/// The question about a turn that was cut off `ago_ms` before the restart found it.
pub fn stale_question(ago_ms: i64) -> Question {
    Question {
        question: format!(
            "This chat was cut off {} ago while working. Continue?",
            duration_words(ago_ms)
        ),
        options: [CONTINUE, STOP].map(String::from).to_vec(),
        multiple: false,
    }
}

/// `text` on one line, at most `max` characters, cut with an ellipsis.
fn one_line(text: &str, max: usize) -> String {
    let line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if line.chars().count() <= max {
        return line;
    }
    let mut cut: String = line.chars().take(max.saturating_sub(1)).collect();
    cut.push('…');
    cut
}

impl Agent {
    /// Settles what a stopped process left of this thread: a review it cut off runs
    /// again, and its running turn goes on, waits for the user, or is stopped.
    pub async fn recover(&self) -> Result<Recovered> {
        self.state
            .recover_goal_verification(&self.thread_id)
            .await?;
        let Some(unfinished) = self.state.unfinished_turn(&self.thread_id).await? else {
            self.pause_phase().await;
            return Ok(Recovered::Nothing);
        };
        let cut_off = self.cut_off(&unfinished).cloned();
        let limit = i64::from(self.config.current().limits().resume_within_seconds) * 1000;
        // A turn that waits for its provider is at work until the wait ends.
        let active_at = match unfinished.turn.waiting {
            Some(TurnWait::Provider { wake_at, .. }) => unfinished.active_at.max(wake_at),
            _ => unfinished.active_at,
        };
        let stale = dusk_protocol::system::now_ms() - active_at > limit;
        let deciding = matches!(unfinished.turn.waiting, Some(TurnWait::Decision { .. }));
        // An agent has no one to ask: its asker learns it was stopped and decides.
        if self.depth > 0 {
            if cut_off.is_some() || stale {
                self.stop_unfinished(None).await?;
                return Ok(Recovered::Nothing);
            }
            return Ok(Recovered::Resume(unfinished.turn));
        }
        if cut_off.is_none() && !stale && !deciding {
            return Ok(Recovered::Resume(unfinished.turn));
        }
        // A provider's wait gives way to the user's decision, which then comes first.
        if matches!(unfinished.turn.waiting, Some(TurnWait::Provider { .. })) {
            self.state.end_wait(&unfinished.turn.turn_id).await?;
        }
        let turn = self.state.wait_for_decision(&self.thread_id).await?;
        let question = match &cut_off {
            Some(call) => cut_off_question(call),
            // Measured to when the wait began, so a second restart asks the same.
            None => {
                let since = match turn.waiting {
                    Some(TurnWait::Decision { since }) => since,
                    _ => dusk_protocol::system::now_ms(),
                };
                stale_question(since - unfinished.active_at)
            }
        };
        info!(
            "thread {} turn {} waits for the user's decision",
            self.thread_id, turn.turn_id
        );
        Ok(Recovered::Decide(turn, question))
    }

    /// Applies the user's answer to the restart's question. Returns the turn when it goes
    /// on; after "Stop" it has ended.
    pub async fn decide(
        &self,
        turn: &Turn,
        answer: &PendingMessage,
        decision: Decision,
    ) -> Result<Option<Turn>> {
        let note = match decision {
            Decision::Stop => {
                self.stop_unfinished(Some(answer.pending_id)).await?;
                return Ok(None);
            }
            Decision::Continue => None,
            Decision::CheckFirst => Some(CHECK_NOTE),
        };
        let unfinished = self.state.unfinished_turn(&self.thread_id).await?;
        let settle = match unfinished.as_ref().and_then(|u| self.cut_off(u)) {
            Some(_) => Settle::CutOff { note },
            None => Settle::AsItStood,
        };
        let turn = self
            .state
            .settle_decision(&turn.turn_id, answer.pending_id, settle)
            .await?;
        info!(
            "thread {} turn {} goes on: {decision:?}",
            self.thread_id, turn.turn_id
        );
        Ok(Some(turn))
    }

    /// Goes on with a turn a restart found: the rest of a wait for its provider is slept
    /// out, the open calls of its last step run, then the turn goes on as any other. A
    /// call that may have taken effect and may not run again never gets here: it was
    /// settled or the turn stopped.
    pub async fn resume_turn(
        &self,
        turn: &Turn,
        inbox: &mut Inbox,
    ) -> Result<(Option<Stop>, TurnStatus)> {
        info!("thread {} turn {} resumes", self.thread_id, turn.turn_id);
        let slept = match turn.waiting {
            Some(TurnWait::Provider { wake_at, .. }) => {
                self.sleep_out(turn, wake_at, inbox).await?
            }
            _ => {
                self.emit(ThreadEvent::TurnRunning(turn.clone()));
                Ok(())
            }
        };
        if let Err(stop) = slept {
            let first = StepEnd::Stopped(stop, TurnOutcome::Interrupted(None));
            return self.run_turn_from(turn, inbox, Some(first)).await;
        }
        let unfinished = self.state.unfinished_turn(&self.thread_id).await?;
        let open = unfinished.as_ref().map_or(&[][..], |u| &u.open[..]);
        ensure!(
            unfinished.as_ref().and_then(|u| self.cut_off(u)).is_none(),
            "turn {} has a call that may not run again",
            turn.turn_id
        );
        let first = match open {
            [] => None,
            open => {
                let config = self.config_now().await;
                Some(self.run_calls(turn, open, &config, inbox).await)
            }
        };
        self.run_turn_from(turn, inbox, first).await
    }

    /// Stops the running turn as a restart always did: an open call gets the result that
    /// says it may have taken effect and may still be running. `answer` is the user's
    /// "Stop", dropped with it. The end is told, so an agent's asker learns it was stopped.
    pub async fn stop_unfinished(&self, answer: Option<i64>) -> Result<()> {
        let turn = match answer {
            Some(answer) => self.state.stop_decided(&self.thread_id, answer).await?,
            None => self.state.interrupt_running_turns(&self.thread_id).await?,
        };
        self.pause_phase().await;
        if let Some(turn) = turn {
            info!("thread {} turn {} stopped", self.thread_id, turn.turn_id);
            self.emit(ThreadEvent::TurnFinished(turn));
        }
        Ok(())
    }

    /// The first open call when it may have taken effect and may not simply run again. A
    /// group of read-only calls is repeatable throughout, and the call after it cannot
    /// have started before the whole group was stored, so the first call decides.
    fn cut_off<'a>(&self, unfinished: &'a Unfinished) -> Option<&'a ToolCall> {
        let first = unfinished.open.first()?;
        (!self.tools.repeatable(&first.name, &first.arguments)).then_some(first)
    }

    /// An agent at work when its turn ended this way is paused where it stopped.
    async fn pause_phase(&self) {
        if self.phase().await.is_some_and(AgentPhase::is_open) {
            self.runtime_phase(AgentPhase::Paused).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn only_an_offered_option_is_a_decision() {
        let call = ToolCall {
            id: "1".into(),
            name: "run_command".into(),
            arguments: json!({ "command": "rm   -rf\nbuild" }),
        };
        let question = cut_off_question(&call);
        assert!(question.check().is_ok());
        // JSON keeps a line break escaped; the runs of spaces go.
        assert_eq!(
            question.question,
            "A restart cut off `run_command`: {\"command\":\"rm -rf\\nbuild\"}. \
             It may have taken effect and may still be running."
        );
        let answer = |option: &str| question.reply(option);
        assert_eq!(
            Decision::of(&question, &answer("Continue")),
            Some(Decision::Continue)
        );
        assert_eq!(
            Decision::of(&question, &answer("Check first")),
            Some(Decision::CheckFirst)
        );
        assert_eq!(
            Decision::of(&question, &answer("Stop")),
            Some(Decision::Stop)
        );
        for other in [
            answer("Continue; and more"),
            answer("(declined)"),
            question.declined("no"),
            "Continue".into(),
            stale_question(5).reply("Continue"),
        ] {
            assert_eq!(Decision::of(&question, &other), None, "{other}");
        }
        let stale = stale_question(2 * 3600 * 1000 + 5 * 60 * 1000);
        assert!(stale.check().is_ok());
        assert_eq!(
            stale.question,
            "This chat was cut off 2h 5m ago while working. Continue?"
        );
        assert_eq!(Decision::of(&stale, &stale.reply("Check first")), None);
    }

    #[test]
    fn long_arguments_are_cut_to_one_line() {
        let long = "x".repeat(200);
        let shown = one_line(&long, SHOWN_ARGUMENTS);
        assert_eq!(shown.chars().count(), SHOWN_ARGUMENTS);
        assert!(shown.ends_with('…'));
        let call = ToolCall {
            id: "1".into(),
            name: "n".repeat(64),
            arguments: json!({ "text": "y".repeat(500) }),
        };
        assert!(cut_off_question(&call).check().is_ok());
    }
}
