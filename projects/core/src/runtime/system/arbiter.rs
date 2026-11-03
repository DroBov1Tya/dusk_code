//! The arbiter: in arbiter mode it answers approval requests in the user's place. It is a
//! thread of its own that the user can open and read: each request is a message there and
//! each verdict its answer. It sees its earlier verdicts and nothing else, runs no tool,
//! and a failure or an answer that is neither yes nor no counts as no.

use std::sync::Arc;

use anyhow::{Context, Result, bail};
use tokio::sync::Mutex;

use super::model_choice::ModelChoice;
use crate::{
    BoxFuture,
    providers::{ChatMessage, ChatRequest, ModelOutput},
    runtime::agent::generation,
    state::{MessageRole, State, TurnAnswer, TurnOutcome},
    tools::{Judge, Request, Verdict},
};

/// The arbiter's thread, one for the whole config.
pub const THREAD: &str = "arbiter";
/// Earlier requests and verdicts it reads before deciding.
const HISTORY: usize = 40;
const MAX_ANSWER_TOKENS: u64 = 400;

pub struct Arbiter {
    state: Arc<State>,
    model: Arc<ModelChoice>,
    /// One request at a time, so its thread reads in order.
    deciding: Mutex<()>,
}

impl Arbiter {
    pub fn new(state: Arc<State>, model: Arc<ModelChoice>) -> Self {
        Self {
            state,
            model,
            deciding: Mutex::new(()),
        }
    }

    async fn decide(&self, request: &Request) -> Result<Verdict> {
        let _one = self.deciding.lock().await;
        if self.state.load_thread(THREAD).await?.is_none() {
            self.state.create_thread(THREAD, THREAD).await?;
            self.state.set_title(THREAD, "arbiter").await?;
        }
        let uid = uuid::Uuid::now_v7().to_string();
        let asked = format!("An agent asks to: {}", request.what());
        self.state.add_user_message(THREAD, &uid, &asked).await?;
        let turn = self
            .state
            .start_turn(THREAD)
            .await?
            .context("the arbiter's turn did not start")?;
        let verdict = self.ask_model().await;
        let answer = match &verdict {
            Ok(verdict) if verdict.allow => format!("ALLOW: {}", verdict.reason),
            Ok(verdict) => format!("DENY: {}", verdict.reason),
            Err(error) => format!("DENY: could not decide: {error:#}"),
        };
        let answer = TurnAnswer {
            content: answer,
            reasoning: None,
        };
        self.state
            .finish_turn(&turn.turn_id, TurnOutcome::Completed(answer))
            .await?;
        verdict
    }

    async fn ask_model(&self) -> Result<Verdict> {
        let choice = self.model.current().context("no model is chosen")?;
        let prompt = crate::release::resources::text("internal/ARBITER.md")?;
        let history = self.state.list_messages(THREAD).await?;
        let skip = history.len().saturating_sub(HISTORY);
        let mut messages = vec![ChatMessage::System(prompt)];
        messages.extend(
            history
                .into_iter()
                .skip(skip)
                .filter_map(|message| match message.role {
                    MessageRole::User => Some(ChatMessage::User(message.content)),
                    MessageRole::Assistant => Some(ChatMessage::Assistant {
                        text: message.content,
                        tool_calls: Vec::new(),
                    }),
                    _ => None,
                }),
        );
        let request = ChatRequest {
            model: &choice.model.id,
            messages: &messages,
            tools: &[],
            max_output_tokens: Some(MAX_ANSWER_TOKENS),
            effort: None,
        };
        let mut output = ModelOutput::default();
        generation::generate(choice.provider.as_ref(), request, &mut output, |_| {}).await?;
        verdict(&output.text)
    }
}

impl Judge for Arbiter {
    fn judge<'a>(&'a self, request: &'a Request) -> BoxFuture<'a, Verdict> {
        Box::pin(self.decide(request))
    }
}

/// `ALLOW` or `DENY` first, then why.
fn verdict(text: &str) -> Result<Verdict> {
    let line = text
        .lines()
        .map(|line| line.trim().trim_start_matches(['*', '#', '`', ' ']))
        .find(|line| !line.is_empty())
        .unwrap_or_default();
    let upper = line.to_uppercase();
    let (allow, rest) = if upper.starts_with("ALLOW") {
        (true, &line[5..])
    } else if upper.starts_with("DENY") {
        (false, &line[4..])
    } else {
        bail!("the arbiter answered neither ALLOW nor DENY");
    };
    let reason = rest
        .trim_start_matches(['*', ':', '-', ' ', '\u{2014}'])
        .trim();
    let reason = match reason.is_empty() {
        true => "no reason given".to_owned(),
        false => reason.to_owned(),
    };
    Ok(Verdict { allow, reason })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        providers::{ChatStream, FinishReason, Model, ModelEvent, ModelProvider},
        runtime::model_choice::Choice,
        testing::temp_state,
    };

    /// Answers every request with the same text.
    struct Says(&'static str);

    struct Once(Vec<ModelEvent>);

    impl ModelProvider for Says {
        fn list_models(&self) -> BoxFuture<'_, Vec<Model>> {
            Box::pin(async { Ok(Vec::new()) })
        }

        fn stream_chat<'a>(&'a self, _: ChatRequest<'a>) -> BoxFuture<'a, Box<dyn ChatStream>> {
            let events = vec![
                ModelEvent::Finished(FinishReason::Stop),
                ModelEvent::Text(self.0.into()),
            ];
            Box::pin(async move { Ok(Box::new(Once(events)) as Box<dyn ChatStream>) })
        }
    }

    impl ChatStream for Once {
        fn next_event(&mut self) -> BoxFuture<'_, Option<ModelEvent>> {
            Box::pin(async move { Ok(self.0.pop()) })
        }
    }

    async fn arbiter(name: &str, says: &'static str) -> Arbiter {
        let model = ModelChoice::new(Choice {
            provider_name: "fake".into(),
            provider: Arc::new(Says(says)),
            model: Model {
                id: "fake".into(),
                context_window: None,
                max_output: None,
            },
        });
        Arbiter::new(Arc::new(temp_state(name).await), Arc::new(model))
    }

    #[tokio::test]
    async fn each_verdict_is_kept_in_its_thread_and_an_unclear_one_denies() {
        let request = Request::Command {
            command: "cargo test".into(),
        };
        let yes = arbiter("arbiter-yes", "ALLOW: runs the tests").await;
        let verdict = yes.judge(&request).await.unwrap();
        assert!(verdict.allow);
        let thread = yes.state.list_messages(THREAD).await.unwrap();
        assert_eq!(thread.len(), 2);
        assert!(thread[0].content.contains("cargo test"));
        assert_eq!(thread[1].content, "ALLOW: runs the tests");
        assert!(yes.state.list_threads().await.unwrap().is_empty());
        let unclear = arbiter("arbiter-unclear", "sure, why not").await;
        assert!(unclear.judge(&request).await.is_err());
        let kept = unclear.state.list_messages(THREAD).await.unwrap();
        assert!(kept[1].content.starts_with("DENY"));
    }

    #[test]
    fn only_a_clear_yes_allows() {
        let allowed = verdict("ALLOW: builds the project in its folder").unwrap();
        assert!(allowed.allow);
        assert_eq!(allowed.reason, "builds the project in its folder");
        assert!(!verdict("**DENY** \u{2014} force push").unwrap().allow);
        assert!(verdict("I think it is fine").is_err());
        assert!(verdict("").is_err());
    }
}
