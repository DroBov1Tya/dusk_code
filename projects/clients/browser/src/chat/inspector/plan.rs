//! The right column of a chat: its plan and tasks, subagents, changed files, background
//! jobs and what the context holds.

use crate::{api, chat::markdown::Markdown, ui::run};
use leptos::prelude::*;

#[component]
pub fn PlanTab(id: String) -> impl IntoView {
    let plan = RwSignal::new(String::new());
    let editing = RwSignal::new(false);
    let draft = RwSignal::new(String::new());
    let thread = id.clone();
    run(async move { api::plan(&thread).await }, move |text| {
        plan.set(text.unwrap_or_default())
    });
    let save_id = id.clone();
    view! {
        <div class="plan">
            <Show when=move || !editing.get()
                fallback=move || view! {
                    <textarea class="plan-edit" prop:value=move || draft.get()
                        on:input=move |ev| draft.set(event_target_value(&ev))></textarea>
                    <div class="row-actions">
                        <button class="primary" on:click={
                            let id = save_id.clone();
                            move |_| {
                                let text = draft.get_untracked();
                                let id = id.clone();
                                run(async move { api::save_plan(&id, text.clone()).await.map(|()| text) },
                                    move |text| { plan.set(text); editing.set(false); });
                            }
                        }>"Save"</button>
                        <button class="ghost" on:click=move |_| editing.set(false)>"Cancel"</button>
                    </div>
                }>
                <Show when=move || !plan.get().is_empty()
                    fallback=|| view! { <p class="muted">"No plan yet. The agent pins one when the work needs it."</p> }>
                    <Markdown text=plan />
                </Show>
                <button class="ghost" on:click=move |_| { draft.set(plan.get_untracked()); editing.set(true); }>"Edit"</button>
            </Show>
        </div>
    }
}
