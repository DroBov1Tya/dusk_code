//! The right column of a chat: its plan and tasks, subagents, changed files, background
//! jobs and what the context holds. The plan tab shows the current plan, the earlier ones
//! under it, and deletes them after a yes.

use crate::{
    api,
    chat::markdown::Markdown,
    ui::{confirm, run},
};
use dusk_protocol::thread::PlanFile;
use leptos::prelude::*;

#[component]
pub fn PlanTab(id: String) -> impl IntoView {
    let plan = RwSignal::new(String::new());
    let plans = RwSignal::new(Vec::<PlanFile>::new());
    let editing = RwSignal::new(false);
    let draft = RwSignal::new(String::new());
    let id = StoredValue::new(id);
    let reload = move || {
        let thread = id.get_value();
        run(
            async move { Ok((api::plan(&thread).await?, api::plans(&thread).await?)) },
            move |(text, list)| {
                plan.set(text.unwrap_or_default());
                plans.set(list);
            },
        );
    };
    reload();
    let delete = move |file: PlanFile| {
        let question = if file.current {
            format!(
                "Delete the current plan \"{}\"? The model reads it with every message; after \
                 this it no longer does. This cannot be undone.",
                file.name
            )
        } else {
            format!("Delete \"{}\"? This cannot be undone.", file.name)
        };
        if confirm(&question) {
            let thread = id.get_value();
            run(
                async move { api::remove_source(&thread, &file.source_id).await },
                move |()| reload(),
            );
        }
    };
    let delete_old = move |_| {
        let old: Vec<PlanFile> = plans
            .get_untracked()
            .into_iter()
            .filter(PlanFile::old)
            .collect();
        let current = old.iter().any(|plan| plan.current);
        let question = format!(
            "Delete {} old plan{}? Plans a newer one replaced and finished ones go{}. This \
             cannot be undone.",
            old.len(),
            if old.len() == 1 { "" } else { "s" },
            if current {
                ", the current plan too: every box of it is ticked"
            } else {
                "; the current plan stays"
            }
        );
        if confirm(&question) {
            let thread = id.get_value();
            run(
                async move { api::remove_old_plans(&thread).await },
                move |_| reload(),
            );
        }
    };
    let current = move || plans.with(|list| list.first().filter(|plan| plan.current).cloned());
    let others = move || {
        plans.with(|list| {
            list.iter()
                .filter(|plan| !plan.current)
                .cloned()
                .collect::<Vec<_>>()
        })
    };
    let old = move || plans.with(|list| list.iter().filter(|plan| plan.old()).count());
    view! {
        <div class="plan">
            <Show when=move || !editing.get()
                fallback=move || view! {
                    <textarea class="plan-edit" prop:value=move || draft.get()
                        on:input=move |ev| draft.set(event_target_value(&ev))></textarea>
                    <div class="row-actions">
                        <button class="primary" on:click=move |_| {
                            let text = draft.get_untracked();
                            let thread = id.get_value();
                            run(async move { api::save_plan(&thread, text).await },
                                move |()| { editing.set(false); reload(); });
                        }>"Save"</button>
                        <button class="ghost" on:click=move |_| editing.set(false)>"Cancel"</button>
                    </div>
                }>
                <Show when=move || !plan.get().is_empty()
                    fallback=|| view! { <p class="muted">"No plan yet. The agent pins one when the work needs it."</p> }>
                    <Markdown text=plan />
                </Show>
                <div class="row-actions">
                    <button class="ghost" on:click=move |_| { draft.set(plan.get_untracked()); editing.set(true); }>"Edit"</button>
                    {move || current().map(|file| view! {
                        <button class="ghost danger" title="Delete this plan; the model stops reading it"
                            on:click=move |_| delete(file.clone())>"Delete"</button>
                    })}
                </div>
                <Show when=move || { !others().is_empty() || old() > 0 }>
                    <div class="plan-older">
                        <div class="muted">"Earlier plans"</div>
                        <For each=others key=|file| file.source_id.clone() let:file>
                            <div class="plan-older-row">
                                <span class="plan-older-name">{file.name.clone()}</span>
                                <span class="muted">{if file.finished { "finished" } else { "replaced" }}</span>
                                <button class="ghost danger" title="Delete this plan"
                                    on:click={let file = file.clone(); move |_| delete(file.clone())}>"Delete"</button>
                            </div>
                        </For>
                        <Show when=move || { old() > 0 }>
                            <div class="row-actions">
                                <button class="ghost" title="Delete the plans a newer one replaced and the finished ones"
                                    on:click=delete_old>
                                    {move || match old() { 1 => "Delete the old plan".to_owned(), n => format!("Delete {n} old plans") }}
                                </button>
                            </div>
                        </Show>
                    </div>
                </Show>
            </Show>
        </div>
    }
}
