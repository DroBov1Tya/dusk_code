//! The working row under a running turn: the mark of what the turn does now and its
//! words, then how long it has taken. The protocol chooses the kind of work
//! (`thread::activity`); here each kind has its look: thinking is a spark that eases from
//! shape to shape beside a word with a light passing through it, writing stands still
//! (the caret at the end of the growing text is the sign), planning writes three lines,
//! a running step is a dot with a ring going out from it, agents are a dot each, waiting
//! for the user is a steady ring, compacting draws three lines together, and a turn the
//! provider holds is a dotted ring. The marks share one slot and the row has its full
//! height from the first frame, so nothing around it moves when the kind changes. All of
//! it stands still when the system asks for less motion.

use super::{
    data::{ChatData, now_ms},
    spark::Spark,
};
use crate::state::Chat;
use dusk_protocol::thread::{Work, WorkKind, duration_words};
use leptos::prelude::*;

/// What the row says, apart from the clock.
#[derive(Debug, PartialEq)]
pub struct Row {
    /// The kind of work and its words, as the protocol chose them.
    pub work: Work,
    /// How long the turn has run; absent when its start is not known.
    pub elapsed: Option<String>,
    /// "18.4k tokens", absent before any call reported usage.
    pub tokens: Option<String>,
    /// "step 7", the call under way; absent when the turn's counts are not known.
    pub step: Option<String>,
}

/// The row at `now` (Unix milliseconds), or none when nothing runs.
pub fn row(chat: &ChatData, now: i64) -> Option<Row> {
    let work = chat.work(now)?;
    // Waiting for the user is not work, and a held turn counts down in its words:
    // nothing counts beside either. Housekeeping has a clock and no steps.
    let (clock, spend) = match work.kind {
        WorkKind::WaitingForUser | WorkKind::Held => (false, false),
        WorkKind::Compacting => (true, false),
        _ => (true, true),
    };
    let spend = chat.spend.filter(|_| spend);
    Some(Row {
        work,
        elapsed: chat
            .turn_started
            .filter(|_| clock)
            .map(|started| duration_words((now - started).max(0))),
        tokens: spend
            .and_then(|spend| spend.tokens())
            .map(|tokens| format!("{tokens} tokens")),
        step: spend.map(|spend| spend.step()),
    })
}

/// The mark of a kind, in the slot every kind shares.
fn mark(kind: WorkKind, stalled: bool) -> AnyView {
    match kind {
        WorkKind::Thinking => view! { <Spark stalled /> }.into_any(),
        WorkKind::Writing => view! { <span class="act-still"></span> }.into_any(),
        WorkKind::Planning => {
            view! { <span class="act-lines"><i></i><i></i><i></i></span> }.into_any()
        }
        WorkKind::Steps => view! { <span class="act-live"></span> }.into_any(),
        WorkKind::Agents { working, started } => {
            // A dot for each agent of the burst, four at most; those done are dim.
            let shown = started.max(working).clamp(1, 4);
            view! {
                <span class="act-dots">
                    {(0..shown).map(|dot| view! { <i class:on=dot < working></i> }).collect_view()}
                </span>
            }
            .into_any()
        }
        WorkKind::WaitingForUser => view! { <span class="act-ring"></span> }.into_any(),
        WorkKind::Compacting => {
            view! { <span class="act-squeeze"><i></i><i></i><i></i></span> }.into_any()
        }
        WorkKind::Held => view! { <span class="act-ring held"></span> }.into_any(),
    }
}

#[component]
pub fn Working(chat: Chat) -> impl IntoView {
    // The clock is read once a second, and not at all while the page is hidden.
    let now = RwSignal::new(now_ms());
    let handle = set_interval_with_handle(
        move || {
            let hidden = web_sys::window()
                .and_then(|w| w.document())
                .is_some_and(|d| d.hidden());
            if !hidden {
                now.set(now_ms());
            }
        },
        std::time::Duration::from_secs(1),
    )
    .ok();
    on_cleanup(move || {
        if let Some(handle) = handle {
            handle.clear();
        }
    });
    let current = Memo::new(move |_| chat.with(|c| row(c, now.get())));
    // Memos of their own, so the mark is drawn anew only when the kind changes and the
    // live region speaks only when the words do.
    let look =
        Memo::new(move |_| current.with(|r| r.as_ref().map(|r| (r.work.kind, r.work.stalled))));
    let words = Memo::new(move |_| {
        current.with(|r| r.as_ref().map(|r| r.work.label.clone()).unwrap_or_default())
    });
    let field =
        move |pick: fn(&Row) -> Option<String>| move || current.with(|r| r.as_ref().and_then(pick));
    let (elapsed, tokens, step) = (
        field(|r| r.elapsed.clone()),
        field(|r| r.tokens.clone()),
        field(|r| r.step.clone()),
    );
    // The words alone are announced; the clock changes every second and is not.
    view! {
        <div class="working-row">
            <span class="act-slot" aria-hidden="true">
                {move || look.get().map(|(kind, stalled)| mark(kind, stalled))}
            </span>
            <span class="sr-only" role="status">{words}</span>
            <span class="working-fields" aria-hidden="true">
                <span class="working-words"
                    class:act-shimmer=move || look.get().is_some_and(|(kind, stalled)| kind == WorkKind::Thinking && !stalled)
                    class:clear=move || look.get().is_some_and(|(kind, _)| kind == WorkKind::WaitingForUser)>
                    {words}
                </span>
                {move || elapsed().map(|elapsed| view! {
                    <span class="working-dot"></span>
                    <span class="working-box num">{elapsed}</span>
                })}
                {move || tokens().map(|tokens| view! {
                    <span class="working-dot"></span>
                    <span class="working-box tokens num">{tokens}</span>
                })}
                {move || step().map(|step| view! {
                    <span class="working-dot"></span>
                    <span class="working-box num">{step}</span>
                })}
            </span>
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dusk_protocol::thread::{ThreadEvent, Turn, TurnStatus, TurnWait};

    fn turn(waiting: Option<TurnWait>) -> Turn {
        Turn {
            turn_id: "t".into(),
            thread_id: "x".into(),
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
    fn a_running_turn_thinks_and_shows_its_elapsed_time() {
        let mut chat = ChatData::default();
        assert_eq!(row(&chat, 0), None);
        chat.apply(ThreadEvent::TurnStarted(turn(None)));
        let shown = row(&chat, 1_000 + 134_000).unwrap();
        assert_eq!(shown.work.kind, WorkKind::Thinking);
        assert_eq!(shown.work.label, "Thinking");
        assert_eq!(shown.elapsed.as_deref(), Some("2m 14s"));
        assert_eq!(shown.step.as_deref(), Some("step 1"));
    }

    #[test]
    fn a_wait_for_the_user_says_so_and_counts_nothing() {
        let mut chat = ChatData::default();
        chat.apply(ThreadEvent::TurnStarted(turn(None)));
        chat.apply(ThreadEvent::TurnWaiting(turn(Some(TurnWait::Decision {
            since: 2_000,
        }))));
        let shown = row(&chat, 6_000).unwrap();
        assert_eq!(shown.work.kind, WorkKind::WaitingForUser);
        assert_eq!(shown.work.label, "Waiting for your decision");
        assert_eq!(
            (shown.elapsed, shown.tokens, shown.step),
            (None, None, None)
        );
        // The next TurnRunning ends the wait.
        chat.apply(ThreadEvent::TurnRunning(turn(None)));
        assert_eq!(row(&chat, 6_000).unwrap().work.kind, WorkKind::Thinking);
    }

    #[test]
    fn compacting_has_a_clock_and_no_steps_until_it_ends() {
        let mut chat = ChatData::default();
        chat.apply(ThreadEvent::TurnStarted(turn(None)));
        chat.apply(ThreadEvent::CompactionStarted);
        let shown = row(&chat, 1_000).unwrap();
        assert_eq!(shown.work.label, "Compacting the context");
        assert_eq!((shown.elapsed.as_deref(), shown.step), (Some("0s"), None));
        chat.apply(ThreadEvent::Idle);
        assert_eq!(row(&chat, 1_000), None);
    }

    #[test]
    fn the_answer_being_written_is_the_one_with_the_caret() {
        use dusk_protocol::model::ModelEvent;
        let mut chat = ChatData::default();
        chat.apply(ThreadEvent::TurnStarted(turn(None)));
        assert_eq!(chat.writing(), None);
        chat.apply(ThreadEvent::Model(ModelEvent::Text("So".into())));
        assert_eq!(chat.writing(), chat.items.last().map(|entry| entry.id));
        assert_eq!(row(&chat, 1_000).unwrap().work.kind, WorkKind::Writing);
        chat.apply(ThreadEvent::Idle);
        assert_eq!(chat.writing(), None);
    }

    #[test]
    fn a_clock_behind_the_core_never_shows_negative_time() {
        let mut chat = ChatData::default();
        chat.apply(ThreadEvent::TurnStarted(turn(None)));
        assert_eq!(row(&chat, 0).unwrap().elapsed.as_deref(), Some("0s"));
    }
}
