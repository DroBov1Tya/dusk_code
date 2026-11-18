//! The Checks block: a snapshot of checks the agent wrote, one row each with a glyph, the
//! state also as a word, and counts. The head says whose it is, so it is not taken for
//! the CI line, which the forge answers through the core.

use dusk_protocol::thread::{Check, CheckState, Checks, Counts};
use leptos::prelude::*;

#[component]
pub fn ChecksView(checks: Checks) -> impl IntoView {
    let summary = checks.summary();
    let described = format!("Checks from the agent: {summary}");
    view! {
        <div class="shown checks" role="group" aria-label=described>
            <div class="checks-head">
                <b>"Checks"</b>
                <span class="meta muted">{summary}</span>
                <span class="grow"></span>
                <span class="meta muted">"from the agent"</span>
            </div>
            {checks.items().iter().cloned().map(|check| view! { <CheckRow check /> }).collect_view()}
        </div>
    }
}

#[component]
fn CheckRow(check: Check) -> impl IntoView {
    let state = check.state;
    let right = right_text(&check);
    view! {
        <div class=format!("check {}", state.name())>
            <Glyph state />
            <span class="check-name">
                <span class="sr-only">{format!("{}: ", state.name())}</span>
                <span class="check-title">{check.name.to_string()}</span>
                {check.detail.map(|line| view! { <span class="check-detail">{line.to_string()}</span> })}
            </span>
            <span class="meta muted num">{right}</span>
        </div>
    }
}

/// What stands at the right of a row: the counts, or else the state as a word.
fn right_text(check: &Check) -> String {
    match check.counts {
        Some(counts) => counts_text(counts),
        None => check.state.name().to_owned(),
    }
}

fn counts_text(counts: Counts) -> String {
    let base = format!("{} / {}", counts.passed(), counts.total());
    match counts.failed() {
        0 => base,
        failed => format!("{base} \u{b7} {failed} failed"),
    }
}

/// The state as a shape, so it is not told by colour alone.
#[component]
fn Glyph(state: CheckState) -> impl IntoView {
    let shape = match state {
        CheckState::Queued => view! { <circle cx="8" cy="8" r="5" /> }.into_any(),
        CheckState::Running => view! {
            <circle class="track" cx="8" cy="8" r="5" />
            <path d="M8 3a5 5 0 0 1 0 10" />
        }
        .into_any(),
        CheckState::Passed => view! { <path d="M3.8 8.4l2.9 2.9 5.5-5.8" /> }.into_any(),
        CheckState::Failed => view! { <path d="M4.8 4.8l6.4 6.4M11.2 4.8l-6.4 6.4" /> }.into_any(),
        CheckState::Skipped => view! { <path d="M4.5 8h7" /> }.into_any(),
    };
    view! {
        <svg class=format!("glyph {}", state.name()) width="16" height="16" viewBox="0 0 16 16" aria-hidden="true">
            {shape}
        </svg>
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dusk_protocol::thread::Line;

    fn check(state: CheckState, counts: Option<Counts>) -> Check {
        Check {
            name: Line::new("unit tests").unwrap(),
            state,
            counts,
            detail: None,
        }
    }

    #[test]
    fn the_right_side_is_counts_or_the_state_as_a_word() {
        let counts = Counts::new(11, 1, 12).unwrap();
        assert_eq!(
            right_text(&check(CheckState::Failed, Some(counts))),
            "11 / 12 \u{b7} 1 failed"
        );
        let clean = Counts::new(214, 0, 214).unwrap();
        assert_eq!(
            right_text(&check(CheckState::Passed, Some(clean))),
            "214 / 214"
        );
        assert_eq!(right_text(&check(CheckState::Skipped, None)), "skipped");
    }
}
