//! The chat's task board: each task with its items, the finished ones folded under
//! "Done (N)" as the desktop app has them.

use crate::{
    state::state,
    ui::{Icon, IconName},
};
use dusk_protocol::thread::Task;
use leptos::prelude::*;

#[component]
pub fn TasksTab(id: String) -> impl IntoView {
    let app = state();
    let tasks = Signal::derive(move || {
        app.chat(&id)
            .map(|c| c.with(|c| c.tasks.clone()))
            .unwrap_or_default()
    });
    let done_open = RwSignal::new(false);
    let split = move || -> (Vec<Task>, Vec<Task>) { tasks.get().into_iter().partition(finished) };
    view! {
        <div class="tasks">
            <Show when=move || !tasks.get().is_empty() fallback=|| view! { <p class="muted">"No tasks."</p> }>
                {move || split().1.into_iter().map(card).collect_view()}
                {move || {
                    let over = split().0;
                    (!over.is_empty()).then(|| view! {
                        <button class="done-fold" aria-expanded=move || done_open.get().to_string()
                            on:click=move |_| done_open.update(|o| *o = !*o)>
                            <Icon name=IconName::ChevronDown />{format!("Done ({})", over.len())}
                        </button>
                        <Show when=move || done_open.get()>
                            {over.clone().into_iter().map(card).collect_view()}
                        </Show>
                    })
                }}
            </Show>
        </div>
    }
}

/// A task is over when the board says so or every item of it is done.
fn finished(task: &Task) -> bool {
    task.done || (!task.items.is_empty() && task.items.iter().all(|item| item.done))
}

fn card(task: Task) -> impl IntoView {
    view! {
        <div class="task" class:done=task.done>
            <div class="task-name">{task.description.clone()}</div>
            <ul>{task.items.iter().map(|item| view! {
                <li class:done=item.done>{item.description.clone()}</li>
            }).collect_view()}</ul>
        </div>
    }
}
