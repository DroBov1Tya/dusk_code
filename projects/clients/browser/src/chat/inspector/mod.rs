//! The right column of a chat: its plan and tasks, subagents, changed files, background
//! jobs and what the context holds.

use crate::{
    state::{View, state},
    ui::{Icon, IconName},
};
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

const TABS: [(Tab, &str, IconName); 6] = [
    (Tab::Plan, "Plan", IconName::Map),
    (Tab::Tasks, "Tasks", IconName::CircleCheck),
    (Tab::Agents, "Agents", IconName::Bot),
    (Tab::Changes, "Changes", IconName::FileText),
    (Tab::Jobs, "Jobs", IconName::SquareTerminal),
    (Tab::Context, "Context", IconName::ChartPie),
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
                    {TABS.map(|(t, label, icon)| view! {
                        <button class="tab" class:active=move || tab.get() == t
                            aria-pressed=move || if tab.get() == t { "true" } else { "false" }
                            on:click=move |_| tab.set(t)>
                            <Icon name=icon />{label}
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
