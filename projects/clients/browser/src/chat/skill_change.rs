//! The row an agent's skill change leaves under its step: what changed and why, the diff
//! on request, and Undo. Everything the core sends is skill data, drawn as text nodes.

use super::blocks::Source;
use crate::{api, state::Chat};
use dusk_protocol::catalog::{DiffKind, SkillChange, SkillDiff, SkillSaveAction};
use leptos::prelude::*;

/// Fetches the thread's recorded changes; live events may have come first, so each is put
/// by its `id` and none is lost.
pub fn load(thread_id: String, chat: Chat) {
    crate::ui::run(
        async move { api::skill_changes(&thread_id).await },
        move |changes| chat.update(|c| c.skills_loaded(changes)),
    );
}

/// The label of a row: what happened to the skill and the reason the agent gave.
pub fn label(change: &SkillChange) -> String {
    let what = match change.action {
        SkillSaveAction::Created => "created",
        SkillSaveAction::Updated => "improved",
        SkillSaveAction::Deleted => "deleted",
    };
    format!("Skill {} {what}: {}", change.skill, change.reason)
}

/// The rows under the step `call_id`.
#[component]
pub fn SkillChanges(call_id: String, source: Source) -> impl IntoView {
    let rows = move || {
        source.with(|c| {
            c.skill_changes_of(&call_id)
                .map(|c| c.id)
                .collect::<Vec<_>>()
        })
    };
    view! {
        <For each=rows key=|id| *id let:id>
            <Row id source />
        </For>
    }
}

#[component]
fn Row(id: i64, source: Source) -> impl IntoView {
    let change =
        Memo::new(move |_| source.with(|c| c.skill_changes.iter().find(|c| c.id == id).cloned()));
    let open = RwSignal::new(false);
    let diff = RwSignal::new(None::<SkillDiff>);
    let busy = RwSignal::new(false);
    let error = RwSignal::new(None::<String>);
    let toggle = move |_| {
        open.update(|o| *o = !*o);
        if open.get_untracked() && diff.with_untracked(Option::is_none) {
            leptos::task::spawn_local(async move {
                match api::skill_diff(id).await {
                    Ok(loaded) => diff.set(Some(loaded)),
                    Err(text) => error.set(Some(text)),
                }
            });
        }
    };
    let undo = move |_| {
        busy.set(true);
        error.set(None);
        leptos::task::spawn_local(async move {
            match api::undo_skill_change(id).await {
                Ok(changes) => {
                    source.update(|c| changes.into_iter().for_each(|ch| c.skill_changed(ch)));
                    // A shown diff is of the change, which undoing does not alter.
                }
                Err(text) => error.set(Some(text)),
            }
            busy.set(false);
        });
    };
    move || {
        let change = change.get()?;
        let undone = change.undone;
        let can_undo = change.can_undo;
        Some(view! {
            <div class="skill-change">
                <div class="skill-change-head">
                    <button class="ghost" aria-expanded=move || open.get().to_string() on:click=toggle>
                        {move || if open.get() { "Hide diff" } else { "Diff" }}
                    </button>
                    <span class="grow">{label(&change)}</span>
                    {undone.then(|| view! { <span class="muted">"undone"</span> })}
                    {(!undone && !can_undo).then(|| view! { <span class="muted">"previous text not kept"</span> })}
                    {can_undo.then(|| view! {
                        <button class="ghost" prop:disabled=move || busy.get() on:click=undo>"Undo"</button>
                    })}
                </div>
                {move || error.get().map(|text| view! { <div class="bad">{text}</div> })}
                <Show when=move || open.get()>
                    <div class="skill-diff">
                        {move || diff.get().map(|d| {
                            let cut = d.cut.then(|| view! { <div class="muted">"more lines not shown"</div> });
                            let lines = d.lines.into_iter().map(|line| {
                                let (class, mark) = match line.kind {
                                    DiffKind::Same => ("same", "  "),
                                    DiffKind::Added => ("added", "+ "),
                                    DiffKind::Removed => ("removed", "- "),
                                };
                                view! { <div class=class>{format!("{mark}{}", line.text)}</div> }
                            }).collect_view();
                            view! { {lines}{cut} }
                        })}
                    </div>
                </Show>
            </div>
        })
    }
}
