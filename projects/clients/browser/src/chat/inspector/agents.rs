//! The right column of a chat: its plan and tasks, subagents, changed files, background
//! jobs and what the context holds.

use crate::chat::{data::AgentState, subagent::state_text};
use crate::state::state;
use leptos::prelude::*;

#[component]
pub fn AgentsTab(id: String) -> impl IntoView {
    let app = state();
    let thread = id.clone();
    let agents = Signal::derive(move || {
        app.chat(&thread)
            .map(|c| c.with(|c| c.agents.clone()))
            .unwrap_or_default()
    });
    view! {
        <div class="agents">
            <Show when=move || !agents.get().is_empty() fallback=|| view! { <p class="muted">"No subagents in this chat."</p> }>
                {let id = id.clone(); move || agents.get().into_iter().map(|agent| {
                    let (parent, thread_id) = (id.clone(), agent.thread_id.clone());
                    let working = agent.state.working();
                    view! {
                        <button class="agent" on:click=move |_| app.subagent.set(Some((parent.clone(), thread_id.clone())))>
                            <FoxBadge working />
                            <div class="agent-text">
                                <div class="agent-name">{agent.name.clone()}<span class="muted">{format!(" · {}", agent.role)}</span></div>
                                <div class="muted">{agent.task.clone()}</div>
                                <div class=agent_class(&agent.state)>{state_text(&agent.state)}</div>
                            </div>
                        </button>
                    }
                }).collect_view()}
            </Show>
        </div>
    }
}

pub fn agent_class(state: &AgentState) -> &'static str {
    match state {
        AgentState::Failed => "agent-state bad",
        AgentState::Done => "agent-state",
        _ => "agent-state live",
    }
}

/// The fox on a subagent's tile; its eyes open while the agent works.
#[component]
pub fn FoxBadge(working: bool) -> impl IntoView {
    view! {
        <svg class="fox" class:awake=working viewBox="0 0 32 32" aria-hidden="true">
            <path d="M3 4l8 5h10l8-5-2 13-9 11h-4L5 17z" class="fox-face" />
            <path d="M12 28l4-7 4 7z" class="fox-nose" />
            <circle cx="11.5" cy="16" r="1.6" class="fox-eye" />
            <circle cx="20.5" cy="16" r="1.6" class="fox-eye" />
        </svg>
    }
}
