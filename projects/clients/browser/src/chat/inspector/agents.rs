//! The right column of a chat: its plan and tasks, subagents, changed files, background
//! jobs and what the context holds.

use crate::chat::subagent::state_text;
use crate::state::state;
use dusk_protocol::{agent::Doing, thread::AgentState};
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
                    let state = agent.state.clone();
                    view! {
                        <button class="agent" on:click=move |_| app.subagent.set(Some((parent.clone(), thread_id.clone())))>
                            <FoxBadge state />
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

/// A still frame of Desktop's fox for the current activity.
#[component]
fn FoxBadge(state: AgentState) -> impl IntoView {
    let frame = match &state {
        AgentState::Thinking => "thinking",
        AgentState::Writing => "writing",
        AgentState::Tool(name) => Doing::of_tool(name).label(),
        AgentState::Done => "done",
        AgentState::Failed => "failed",
    };
    let href = format!("#dusk-fox-{frame}");
    view! {
        <svg class="fox" class:working=state.working() viewBox="0 0 22 22" aria-hidden="true" focusable="false">
            <use href=href />
        </svg>
    }
}
