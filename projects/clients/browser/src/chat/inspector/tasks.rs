//! The right column of a chat: its plan and tasks, subagents, changed files, background
//! jobs and what the context holds.

use crate::state::state;
use leptos::prelude::*;

#[component]
pub fn TasksTab(id: String) -> impl IntoView {
    let app = state();
    let tasks = Signal::derive(move || {
        app.chat(&id)
            .map(|c| c.with(|c| c.tasks.clone()))
            .unwrap_or_default()
    });
    view! {
        <div class="tasks">
            <Show when=move || !tasks.get().is_empty() fallback=|| view! { <p class="muted">"No tasks."</p> }>
                {move || tasks.get().into_iter().map(|task| view! {
                    <div class="task" class:done=task.done>
                        <div class="task-name">{task.description.clone()}</div>
                        <ul>{task.items.iter().map(|item| view! {
                            <li class:done=item.done>{item.description.clone()}</li>
                        }).collect_view()}</ul>
                    </div>
                }).collect_view()}
            </Show>
        </div>
    }
}
