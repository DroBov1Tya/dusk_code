//! The judge of a stall candidate: one call to a model, without tools, that reads a
//! digest of the turn and says whether it is stuck (`repeats::judge`). There is no
//! smaller helper model in the config to send it to (titles and summaries use the chat's
//! model too), so it goes to the turn's own model, at no extra effort.

use super::{Agent, Pace};
use crate::{
    providers::{ChatMessage, ChatRequest, ModelOutput},
    runtime::{
        ThreadEvent,
        agent::{Inbox, Stop, generation, repeats},
    },
    state::{MessageRole, Turn},
};
use dusk_protocol::thread::RepeatRung;
use tracing::{info, warn};

/// The judge answers with one short JSON object.
const MAX_JUDGE_TOKENS: u64 = 300;

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
        let Some(choice) = self.model.current() else {
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
        let request = ChatRequest {
            model: &choice.model.id,
            messages: &messages,
            tools: &[],
            max_output_tokens: Some(MAX_JUDGE_TOKENS),
            effort: None,
            cache_key: None,
        };
        let idle = std::time::Duration::from_secs(
            self.config.current().limits().model_timeout_seconds.into(),
        );
        let mut output = ModelOutput::default();
        let generated = generation::generate(&*choice.provider, request, &mut output, idle, |_| {});
        if let Err(error) = inbox.race(generated).await? {
            warn!("the judge's call failed: {error:#}");
            return Ok(None);
        }
        if let Some(usage) = output.usage {
            info!(
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
