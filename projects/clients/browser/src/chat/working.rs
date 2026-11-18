//! The working row under a running turn: what the turn does now and how long it has
//! taken. Each field has a box of fixed width and the row its full height from the first
//! frame, so nothing around it moves while the numbers change. The turn's tokens and step
//! are not known to the client yet; a field with nothing to show is absent, not zero.

use super::data::{ChatData, now_ms};
use crate::state::Chat;
use dusk_protocol::thread::{TurnWait, duration_words};
use leptos::prelude::*;

/// What the row says, apart from the clock.
#[derive(Debug, PartialEq)]
pub struct Row {
    /// "Working", or what the turn waits for or does instead.
    pub words: String,
    /// How long the turn has run; absent when its start is not known.
    pub elapsed: Option<String>,
    /// The figure stands still while the turn waits for the user.
    pub still: bool,
}

/// The row at `now` (Unix milliseconds): a retry replaces the whole row as it does today,
/// a wait or a compaction replaces the word "Working" and the elapsed time stays.
pub fn row(chat: &ChatData, now: i64) -> Row {
    let elapsed = chat
        .turn_started
        .map(|started| duration_words((now - started).max(0)));
    if let Some(line) = chat.retry_line(now) {
        return Row {
            words: line,
            elapsed: None,
            still: false,
        };
    }
    let (words, still) = match chat.waiting {
        Some(wait) => (
            wait.text(Some(now)),
            matches!(wait, TurnWait::Decision { .. }),
        ),
        None if chat.compacting => ("Compacting the context".to_owned(), false),
        None => ("Working".to_owned(), false),
    };
    Row {
        words,
        elapsed,
        still,
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
    // A memo of its own, so the live region changes only when the words do.
    let words = Memo::new(move |_| current.with(|r| r.words.clone()));
    // The words alone are announced; the clock changes every second and is not.
    view! {
        <div class="working-row">
            <span class="working-figure" class:still=move || current.with(|r| r.still) aria-hidden="true">
                <span class="spinner"></span>
            </span>
            <span class="sr-only" role="status">{words}</span>
            <span class="working-fields" aria-hidden="true">
                <span class="working-words">{words}</span>
                {move || current.with(|r| r.elapsed.clone()).map(|elapsed| view! {
                    <span class="working-dot"></span>
                    <span class="working-box num">{elapsed}</span>
                })}
            </span>
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dusk_protocol::thread::ThreadEvent;
    use dusk_protocol::thread::{Turn, TurnStatus};

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
        }
    }

    #[test]
    fn a_running_turn_shows_working_and_its_elapsed_time() {
        let mut chat = ChatData::default();
        chat.apply(ThreadEvent::TurnStarted(turn(None)));
        let shown = row(&chat, 1_000 + 134_000);
        assert_eq!(shown.words, "Working");
        assert_eq!(shown.elapsed.as_deref(), Some("2m 14s"));
        assert!(!shown.still);
    }

    #[test]
    fn a_wait_for_the_user_replaces_the_word_and_stills_the_figure() {
        let mut chat = ChatData::default();
        chat.apply(ThreadEvent::TurnStarted(turn(None)));
        chat.apply(ThreadEvent::TurnWaiting(turn(Some(TurnWait::Decision {
            since: 2_000,
        }))));
        let shown = row(&chat, 6_000);
        assert_eq!(shown.words, "Waiting for your decision");
        assert_eq!(shown.elapsed.as_deref(), Some("5s"));
        assert!(shown.still);
        // The next TurnRunning ends the wait.
        chat.apply(ThreadEvent::TurnRunning(turn(None)));
        assert_eq!(row(&chat, 6_000).words, "Working");
    }

    #[test]
    fn compacting_replaces_the_word_until_it_ends() {
        let mut chat = ChatData::default();
        chat.apply(ThreadEvent::TurnStarted(turn(None)));
        chat.apply(ThreadEvent::CompactionStarted);
        assert_eq!(row(&chat, 1_000).words, "Compacting the context");
        chat.apply(ThreadEvent::Idle);
        assert_eq!(row(&chat, 1_000).words, "Working");
        assert_eq!(row(&chat, 1_000).elapsed, None);
    }

    #[test]
    fn a_clock_behind_the_core_never_shows_negative_time() {
        let mut chat = ChatData::default();
        chat.apply(ThreadEvent::TurnStarted(turn(None)));
        assert_eq!(row(&chat, 0).elapsed.as_deref(), Some("0s"));
    }
}
