//! The last row of the transcript while a turn runs: the working figure and what the turn
//! does now, with how long it has run and which step it is on, in boxes of fixed width so
//! nothing moves as the numbers tick. A model call tried again replaces the words.

use dusk_protocol::{system::now_ms, thread::TurnWait};
use gpui_kit::{component::ActiveTheme, *};

use crate::{
    pages::chat::{
        timing,
        transcript::Transcript,
        working::{self, tabular},
    },
    ui::Type,
};

/// The row's height, which is the figure's: it has all of it from its first frame.
const HEIGHT: f32 = 28.;
/// Boxes of the counts. The tokens of the turn go between them (96 px) once the core
/// reports them.
const ELAPSED_BOX: f32 = 56.;
const STEP_BOX: f32 = 56.;

/// What the row says, apart from how it is drawn.
#[derive(Debug, PartialEq, Eq)]
pub struct Status {
    pub words: String,
    pub elapsed: Option<String>,
    pub step: Option<String>,
    /// The turn waits for the user: the figure stands still.
    pub still: bool,
}

/// The row at `now` (Unix milliseconds). The words of a wait are the protocol's.
pub fn status(chat: &Transcript, now: i64) -> Status {
    let decision = matches!(chat.waiting, Some(TurnWait::Decision { .. }));
    let began = chat.turn_started.or_else(|| {
        chat.span(chat.activity_boundaries.len())
            .map(|(began, _)| began)
    });
    Status {
        words: match chat.waiting {
            Some(wait) => wait.text(Some(now)),
            None => "Working".to_owned(),
        },
        // Waiting for the user is not work: nothing counts.
        elapsed: began
            .filter(|_| !decision)
            .map(|began| timing::duration(now - began)),
        // The call under way is the one after those that finished.
        step: chat
            .steps
            .filter(|_| !decision)
            .map(|done| format!("step {}", done.saturating_add(1))),
        still: decision,
    }
}

pub fn render(shown: &str, chat: &Transcript, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let id = SharedString::from(format!("working-{shown}"));
    let row = div()
        .h(px(HEIGHT))
        .flex()
        .flex_row()
        .items_center()
        .gap_3()
        .meta()
        .text_color(theme.muted_foreground);
    if let Some((retry, arrived)) = &chat.retry {
        let waited = u64::try_from(arrived.elapsed().as_millis()).unwrap_or(u64::MAX);
        return row
            .child(working::figure(
                id,
                theme.primary,
                theme.foreground,
                false,
                cx,
            ))
            .child(div().min_w_0().truncate().child(retry.line(waited)))
            .into_any_element();
    }
    let status = status(chat, now_ms());
    let field = |text: String, width: f32| {
        div()
            .flex_none()
            .w(px(width))
            .font_features(tabular())
            .child(text)
    };
    let dot = || div().flex_none().px_2().child("\u{00B7}");
    let mut counts = div().flex().flex_row().items_center();
    for (text, width) in [(status.elapsed, ELAPSED_BOX), (status.step, STEP_BOX)] {
        if let Some(text) = text {
            counts = counts.child(dot()).child(field(text, width));
        }
    }
    row.child(working::figure(
        id,
        theme.primary,
        theme.foreground,
        status.still,
        cx,
    ))
    .child(
        div()
            .flex()
            .flex_row()
            .items_center()
            .min_w_0()
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .font_features(tabular())
                    .child(status.words),
            )
            .child(counts),
    )
    .into_any_element()
}

#[cfg(test)]
mod tests {
    use dusk_protocol::{
        model::{FinishReason, ModelEvent, ProviderWaitKind},
        thread::{ThreadEvent, Turn, TurnStatus, TurnWait},
    };

    use super::{Status, status};
    use crate::pages::chat::transcript::Transcript;

    fn turn(waiting: Option<TurnWait>) -> Turn {
        Turn {
            turn_id: "t".into(),
            thread_id: "c".into(),
            last_sequence: 1,
            response_message_id: None,
            status: TurnStatus::Running,
            started_at: 1_000,
            finished_at: None,
            error_message: None,
            waiting,
        }
    }

    #[test]
    fn a_running_turn_counts_time_and_the_step_under_way() {
        let mut chat = Transcript::default();
        chat.apply(ThreadEvent::TurnStarted(turn(None)));
        assert_eq!(
            status(&chat, 135_000),
            Status {
                words: "Working".into(),
                elapsed: Some("2m 14s".into()),
                step: Some("step 1".into()),
                still: false,
            }
        );
        chat.apply(ThreadEvent::Model(ModelEvent::Finished(
            FinishReason::ToolCalls,
        )));
        assert_eq!(status(&chat, 135_000).step.as_deref(), Some("step 2"));
    }

    #[test]
    fn a_turn_joined_midway_shows_no_step() {
        let mut chat = Transcript::default();
        chat.apply(ThreadEvent::TurnRunning(turn(None)));
        let now = status(&chat, 9_000);
        assert_eq!(now.elapsed.as_deref(), Some("8s"));
        assert_eq!(now.step, None);
    }

    #[test]
    fn waits_use_the_protocols_words_and_the_decision_wait_stands_still() {
        let mut chat = Transcript::default();
        chat.apply(ThreadEvent::TurnStarted(turn(None)));
        let provider = TurnWait::Provider {
            kind: ProviderWaitKind::try_from(dusk_protocol::model::ProviderErrorKind::RateLimited)
                .unwrap(),
            since: 1_000,
            wake_at: 61_000,
        };
        chat.apply(ThreadEvent::TurnWaiting(turn(Some(provider))));
        let waiting = status(&chat, 11_000);
        assert_eq!(waiting.words, provider.text(Some(11_000)));
        assert!(waiting.elapsed.is_some() && !waiting.still);

        let decision = TurnWait::Decision { since: 2_000 };
        chat.apply(ThreadEvent::TurnWaiting(turn(Some(decision))));
        assert_eq!(
            status(&chat, 11_000),
            Status {
                words: "Waiting for your decision".into(),
                elapsed: None,
                step: None,
                still: true,
            }
        );
        // The turn runs again, then ends: nothing of the wait is left.
        chat.apply(ThreadEvent::TurnRunning(turn(None)));
        assert_eq!(status(&chat, 11_000).words, "Working");
        chat.apply(ThreadEvent::Idle);
        assert!(chat.waiting.is_none() && chat.turn_started.is_none());
    }
}
