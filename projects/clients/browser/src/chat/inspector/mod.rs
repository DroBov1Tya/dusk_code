//! The right column of a chat: its plan and tasks, subagents, changed files, background
//! jobs and what the context holds.

use crate::state::{View, state};
use leptos::prelude::*;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Plan,
    Tasks,
    Agents,
    Changes,
    Jobs,
    Context,
}

const TABS: [(Tab, &str); 6] = [
    (Tab::Plan, "Plan"),
    (Tab::Tasks, "Tasks"),
    (Tab::Agents, "Agents"),
    (Tab::Changes, "Changes"),
    (Tab::Jobs, "Jobs"),
    (Tab::Context, "Context"),
];

#[component]
pub fn Inspector() -> impl IntoView {
    let app = state();
    let tab = RwSignal::new(Tab::Tasks);
    // Only an open chat has anything to inspect.
    let thread = move || match app.view.get() {
        View::Chat(Some(id)) => Some(id),
        _ => None,
    };
    move || {
        let id = thread()?;
        Some(view! {
            <aside class="inspector">
                <div class="tabs">
                    {TABS.map(|(t, label)| view! {
                        <button class="tab" class:active=move || tab.get() == t on:click=move |_| tab.set(t)>
                            {label}
                        </button>
                    })}
                </div>
                <div class="tab-body">
                    {let id = id.clone(); move || match tab.get() {
                        Tab::Plan => view! { <PlanTab id=id.clone() /> }.into_any(),
                        Tab::Tasks => view! { <TasksTab id=id.clone() /> }.into_any(),
                        Tab::Agents => view! { <AgentsTab id=id.clone() /> }.into_any(),
                        Tab::Changes => view! { <ChangesTab id=id.clone() /> }.into_any(),
                        Tab::Jobs => view! { <JobsTab id=id.clone() /> }.into_any(),
                        Tab::Context => view! { <ContextTab id=id.clone() /> }.into_any(),
                    }}
                </div>
            </aside>
        })
    }
}

mod agents;
mod changes;
mod context;
mod jobs;
mod plan;
mod tasks;

use agents::AgentsTab;
use changes::ChangesTab;
use context::ContextTab;
use jobs::JobsTab;
use plan::PlanTab;
use tasks::TasksTab;
