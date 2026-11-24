//! The goal strip above the composer, while the chat has a goal: folded, the indicator
//! ("◎ goal active · 4m 2s") and the condition in a line, with Clear; unfolded, the card
//! with the condition, the time, the turns judged, the tokens spent, why it is paused and
//! the evaluator's newest reason. Clear sends `/goal clear`, as typing it would.

use super::data::now_ms;
use crate::{
    state::{Chat, state},
    ui::{Icon, IconName},
};
use dusk_protocol::thread::{Goal, GoalStatus, duration_words, plural, short_count};
use leptos::prelude::*;

#[component]
pub fn GoalStrip(id: String, chat: Chat) -> impl IntoView {
    let goal = Memo::new(move |_| chat.with(|c| c.goal.clone()));
    view! {
        <Show when=move || goal.with(Option::is_some)>
            <Strip id=id.clone() goal />
        </Show>
    }
}

/// The strip itself, drawn only while there is a goal, so its clock runs only then.
#[component]
fn Strip(id: String, goal: Memo<Option<Goal>>) -> impl IntoView {
    let app = state();
    let open = RwSignal::new(false);
    // The elapsed time is read once a second, and not at all while the page is hidden.
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
    let paused = move || goal.with(|g| g.as_ref().is_some_and(|g| g.status == GoalStatus::Paused));
    let indicator = move || {
        let now = now.get();
        goal.with(|g| g.as_ref().map(|g| g.indicator(now)).unwrap_or_default())
    };
    let text = move || goal.with(|g| g.as_ref().map(|g| g.text.clone()).unwrap_or_default());
    let clear = move |_| app.send(Some(id.clone()), "/goal clear".into(), false, Vec::new());
    view! {
        <div class="goal-strip">
            <div class="goal-head">
                <button class="goal-line" aria-expanded=move || open.get().to_string()
                    title="Show the goal" on:click=move |_| open.update(|o| *o = !*o)>
                    <Icon name=IconName::ChevronDown />
                    <span class="goal-state" class:warn=paused>{indicator}</span>
                    <span class="goal-text">{text}</span>
                </button>
                <button class="ghost quiet goal-clear" title="Clear the goal (/goal clear)" on:click=clear>
                    "Clear"
                </button>
            </div>
            <Show when=move || open.get()>
                {move || goal.get().map(|g| card(g, now.get()))}
            </Show>
        </div>
    }
}

/// What `/goal` alone says, as fields.
fn card(goal: Goal, now: i64) -> impl IntoView {
    let mut fields = vec![
        ("Condition", goal.text),
        ("Elapsed", duration_words((now - goal.started_at).max(0))),
        ("Evaluated", plural(u64::from(goal.turns), "turn")),
        ("Tokens", short_count(goal.tokens)),
    ];
    if let Some(note) = goal.note {
        fields.push(("Paused", note));
    }
    if let Some(reason) = goal.reason {
        fields.push(("Latest", reason));
    }
    let fields = fields
        .into_iter()
        .map(|(label, value)| {
            view! { <div class="goal-field"><span class="muted">{label}</span><span>{value}</span></div> }
        })
        .collect_view();
    view! { <div class="goal-card">{fields}</div> }
}
