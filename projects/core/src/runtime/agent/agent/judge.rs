//! The judge of a stall candidate: one call to a model, without tools, that reads a
//! digest of the turn and says whether it is stuck (`repeats::judge`). It goes to the
//! helper model the user set for the arbiter (`arbiter.model`), as a smaller model will
//! do, and to the turn's own model when none is set or it cannot be had. It thinks at low
//! effort where the model takes one, so its thinking leaves room for the answer.

use super::{Agent, Pace};
use crate::{
    config::Effort,
    providers::{ChatMessage, ChatRequest, ModelOutput, is_refused_effort, is_unsupported_effort},
    runtime::{
        ThreadEvent,
        agent::{Inbox, Stop, generation, repeats},
        model_choice::Choice,
    },
    state::{MessageRole, Turn},
};
use dusk_protocol::thread::RepeatRung;
use tracing::{info, warn};

/// The judge answers with one short JSON object; the rest is room for its thinking.
const MAX_JUDGE_TOKENS: u64 = 2048;

/// What the judge made of a stall candidate.
pub(super) enum Judged {
    /// The turn is stuck: it ends as failed, with why.
    Stuck(String),
    /// The turn goes on, and the next step reads this note.
    Redirect(String),
    /// The turn goes on as it was.
    Goes,
    /// The user stopped the turn while the judge read it.
    Stopped(Stop),
}

impl Agent {
    /// Reads the stall candidate `stall` of `turn` with a judge model. A failed call or
    /// an answer that cannot be read lets the turn go on; either way the next candidate
    /// waits longer.
    pub(super) async fn judge_stall(
        &self,
        turn: &Turn,
        stall: &repeats::Stall,
        inbox: &mut Inbox,
    ) -> Judged {
        info!(turn = %turn.turn_id, count = stall.count, what = %stall.what, "judging a stall");
        let digest = self.repeats.lock().digest(&turn.turn_id);
        let judged = self.ask_judge(turn, stall, &digest, inbox).await;
        self.repeats.lock().judged(&turn.turn_id);
        let verdict = match judged {
            Ok(Some(verdict)) => verdict,
            Ok(None) => return Judged::Goes,
            Err(stop) => return Judged::Stopped(stop),
        };
        info!(
            turn = %turn.turn_id,
            stuck = verdict.stuck,
            confidence = verdict.confidence,
            "the judge answered"
        );
        if verdict.sure_stuck() {
            self.emit(ThreadEvent::RepeatNotice {
                rung: RepeatRung::JudgeStop,
                count: stall.count,
            });
            return Judged::Stuck(repeats::judge_stop(stall, &verdict, &digest));
        }
        if verdict.redirect.is_empty() {
            return Judged::Goes;
        }
        self.emit(ThreadEvent::RepeatNotice {
            rung: RepeatRung::JudgeRedirect,
            count: stall.count,
        });
        // The step that takes the advice up thinks at the effort set.
        *self.pace.lock() = Pace::Rethink;
        Judged::Redirect(repeats::judge_note(stall, &verdict.redirect))
    }

    /// The judge's verdict; None when it could not be asked or did not answer in shape.
    async fn ask_judge(
        &self,
        turn: &Turn,
        stall: &repeats::Stall,
        digest: &repeats::Digest,
        inbox: &mut Inbox,
    ) -> Result<Option<repeats::Verdict>, Stop> {
        let Some(choice) = self.judge_model().await else {
            warn!("no model to judge a stall with");
            return Ok(None);
        };
        let prompt = match crate::release::resources::text("internal/LOOP_JUDGE.md") {
            Ok(prompt) => prompt,
            Err(error) => {
                warn!("cannot read the judge's prompt: {error:#}");
                return Ok(None);
            }
        };
        let request = match self.request_of(turn).await {
            Ok(request) => request,
            Err(error) => {
                warn!("cannot read the turn's request: {error:#}");
                String::new()
            }
        };
        let messages = [
            ChatMessage::System(prompt.trim().to_owned()),
            ChatMessage::User(repeats::digest(&request, stall, digest)),
        ];
        let idle = std::time::Duration::from_secs(
            self.config.current().limits().model_timeout_seconds.into(),
        );
        let mut output = ModelOutput::default();
        // Low effort first; a model that says it takes no effort, as one without
        // reasoning does, is asked again without one (`RefusedEffort`), and so is a
        // subagent's model that refused it before.
        let refused = self.effort_refused.lock().as_deref() == Some(choice.model.id.as_str());
        let efforts = if refused {
            &[None][..]
        } else {
            &[Some(Effort::Low), None][..]
        };
        for &effort in efforts {
            let request = ChatRequest {
                model: &choice.model.id,
                messages: &messages,
                tools: &[],
                max_output_tokens: Some(MAX_JUDGE_TOKENS),
                effort,
                cache_key: None,
            };
            output = ModelOutput::default();
            let generated =
                generation::generate(&*choice.provider, request, &mut output, idle, |_| {});
            match inbox.race(generated).await? {
                Ok(_) => break,
                Err(error)
                    if effort.is_some()
                        && (is_unsupported_effort(&error) || is_refused_effort(&error)) =>
                {
                    info!("the judge's model takes no reasoning effort; asking without it");
                    continue;
                }
                Err(error) => {
                    warn!("the judge's call failed: {error:#}");
                    return Ok(None);
                }
            }
        }
        // Not in the usage ledger: its purposes are fixed by the table (answer, compact,
        // title), and an answer would count as a step of the turn.
        if let Some(usage) = output.usage {
            info!(
                model = %choice.model.id,
                prompt = usage.prompt_tokens,
                completion = usage.completion_tokens,
                "judge call"
            );
        }
        let verdict = repeats::verdict(&output.text);
        if verdict.is_none() {
            warn!("the judge's answer could not be read");
        }
        Ok(verdict)
    }

    /// The arbiter's helper model when the user set one and it can be had, else the
    /// turn's own.
    async fn judge_model(&self) -> Option<Choice> {
        let config = self.config.current();
        if let Some(name) = &config.arbiter().model {
            match Choice::pick(&config, Some(name)).await {
                Ok(choice) => return Some(choice),
                Err(error) => warn!("cannot judge on {name}, the turn's model will: {error:#}"),
            }
        }
        self.model.current()
    }

    /// The newest message from the user, or another agent, the turn answers.
    async fn request_of(&self, turn: &Turn) -> anyhow::Result<String> {
        let messages = self
            .state
            .messages_from(&self.thread_id, turn.last_sequence, 1)
            .await?;
        Ok(messages
            .into_iter()
            .find(|message| matches!(message.role, MessageRole::User | MessageRole::Job))
            .map(|message| message.content)
            .unwrap_or_default())
    }
}
