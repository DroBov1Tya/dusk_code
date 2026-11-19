//! The last row of the transcript while a turn runs: the mark of what the turn does now
//! and its words (`pages/chat/activity` draws both, the protocol chooses them), then how
//! long it has run, the tokens it used and which step it is on, in tabular figures so
//! nothing moves as the numbers tick. The core counts the tokens and steps.

use dusk_protocol::{
    system::now_ms,
    thread::{Work, WorkKind},
};
use gpui_kit::{component::ActiveTheme, *};

use crate::{
    pages::chat::{activity, timing, transcript::Transcript, working::tabular},
    ui::Type,
};

/// The row's height: it has all of it from its first frame, whatever the kind.
const HEIGHT: f32 = 28.;

/// What the row says, apart from how it is drawn.
#[derive(Debug, PartialEq, Eq)]
pub struct Status {
    pub work: Work,
    pub elapsed: Option<String>,
    /// Absent, not zero, before any call reported usage.
    pub tokens: Option<String>,
    pub step: Option<String>,
}

/// The row at `now` (Unix milliseconds), or none when nothing runs.
pub fn status(chat: &Transcript, now: i64) -> Option<Status> {
    let work = chat.work(now)?;
    let began = chat.turn_started.or_else(|| {
        chat.working
            .then(|| chat.span(chat.activity_boundaries.len()))
            .flatten()
            .map(|(began, _)| began)
    });
    // Waiting for the user is not work, and a held turn counts down in its words:
    // nothing counts beside either. Housekeeping has a clock and no steps.
    let (clock, spend) = match work.kind {
        WorkKind::WaitingForUser | WorkKind::Held => (false, false),
        WorkKind::Compacting => (true, false),
        _ => (true, true),
    };
    let spend = chat.spend.filter(|_| spend);
    Some(Status {
        work,
        elapsed: began
            .filter(|_| clock)
            .map(|began| timing::duration(now - began)),
        tokens: spend
            .and_then(|spend| spend.tokens())
            .map(|tokens| format!("{tokens} tokens")),
        step: spend.map(|spend| spend.step()),
    })
}

/// "2m 4s" as "2m 04s": the part after the first keeps two digits, so the text is as
/// wide at every second of a minute and what follows it does not step sideways.
fn steady(time: &str) -> String {
    let mut parts = time.split(' ');
    let mut out = parts.next().unwrap_or_default().to_owned();
    for part in parts {
        out.push(' ');
        if part.chars().take_while(char::is_ascii_digit).count() == 1 {
            out.push('0');
        }
        out.push_str(part);
    }
    out
}

pub fn render(chat: &Transcript, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let row = div()
        .h(px(HEIGHT))
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .meta()
        .text_color(theme.muted_foreground);
    let Some(status) = status(chat, now_ms()) else {
        return row.into_any_element();
    };
    // Every count is as wide as its own text and the dots stand at one distance from
    // both neighbours. Nothing moves as the numbers tick: the digits are tabular, and
    // the second part of a time keeps two of them (`steady`).
    let dot = || div().flex_none().px_2().child("\u{00B7}");
    let mut counts = div()
        .flex()
        .flex_none()
        .flex_row()
        .items_center()
        .font_features(tabular());
    let fields = [
        status.elapsed.map(|time| steady(&time)),
        status.tokens,
        status.step,
    ];
    for text in fields.into_iter().flatten() {
        counts = counts.child(dot()).child(div().flex_none().child(text));
    }
    row.child(activity::mark(&status.work, cx))
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
                        .child(activity::words(&status.work, cx)),
                )
                .child(counts),
        )
        // The clock and a countdown tick while the mark stands still.
        .child(activity::second_tick())
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use dusk_protocol::{
        model::ProviderWaitKind,
        thread::{ThreadEvent, Turn, TurnStatus, TurnWait, WorkKind},
    };

    use super::{status, steady};

    #[test]
    fn a_time_keeps_its_width_through_a_minute() {
        assert_eq!(steady("9s"), "9s");
        assert_eq!(steady("2m 4s"), "2m 04s");
        assert_eq!(steady("2m 14s"), "2m 14s");
        assert_eq!(steady("1h 5m"), "1h 05m");
    }

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
            steps: 0,
            tokens: 0,
            cost_micro_usd: 0,
        }
    }

    #[test]
    fn a_running_turn_counts_time_and_the_step_under_way() {
        let mut chat = Transcript::default();
        chat.apply(ThreadEvent::TurnStarted(turn(None)));
        let running = status(&chat, 135_000).unwrap();
        assert_eq!(running.work.kind, WorkKind::Thinking);
        assert_eq!(running.elapsed.as_deref(), Some("2m 14s"));
        assert_eq!(running.tokens, None);
        assert_eq!(running.step.as_deref(), Some("step 1"));
        chat.apply(ThreadEvent::TurnProgress {
            turn_id: "t".into(),
            steps: 6,
            tokens: 18_420,
            cost_micro_usd: 0,
        });
        let now = status(&chat, 135_000).unwrap();
        assert_eq!(now.tokens.as_deref(), Some("18.4k tokens"));
        assert_eq!(now.step.as_deref(), Some("step 7"));
    }

    #[test]
    fn a_held_turn_counts_down_and_a_wait_for_the_user_counts_nothing() {
        let mut chat = Transcript::default();
        chat.apply(ThreadEvent::TurnStarted(turn(None)));
        let provider = TurnWait::Provider {
            kind: ProviderWaitKind::try_from(dusk_protocol::model::ProviderErrorKind::RateLimited)
                .unwrap(),
            since: 1_000,
            wake_at: 61_000,
        };
        chat.apply(ThreadEvent::TurnWaiting(turn(Some(provider))));
        let held = status(&chat, 11_000).unwrap();
        assert_eq!(held.work.kind, WorkKind::Held);
        assert_eq!(held.work.label, provider.text(Some(11_000)));
        assert_eq!((held.elapsed, held.step), (None, None));

        let decision = TurnWait::Decision { since: 2_000 };
        chat.apply(ThreadEvent::TurnWaiting(turn(Some(decision))));
        let waiting = status(&chat, 11_000).unwrap();
        assert_eq!(waiting.work.kind, WorkKind::WaitingForUser);
        assert_eq!(waiting.work.label, "Waiting for your decision");
        assert_eq!(
            (waiting.elapsed, waiting.tokens, waiting.step),
            (None, None, None)
        );
        // The turn runs again, then ends: nothing of the wait is left, and no row.
        chat.apply(ThreadEvent::TurnRunning(turn(None)));
        assert_eq!(status(&chat, 11_000).unwrap().work.kind, WorkKind::Thinking);
        chat.apply(ThreadEvent::Idle);
        assert!(chat.waiting.is_none() && chat.turn_started.is_none());
        assert_eq!(status(&chat, 11_000), None);
    }

    #[test]
    fn agents_that_work_on_after_the_turn_keep_the_row_without_a_clock() {
        let mut chat = Transcript::default();
        chat.apply(ThreadEvent::TurnStarted(turn(None)));
        chat.apply(ThreadEvent::AgentsWorking(true));
        chat.apply(ThreadEvent::Idle);
        let agents = status(&chat, 11_000).unwrap();
        assert!(matches!(agents.work.kind, WorkKind::Agents { .. }));
        assert_eq!(agents.elapsed, None);
        chat.apply(ThreadEvent::AgentsWorking(false));
        assert_eq!(status(&chat, 11_000), None);
    }
}
