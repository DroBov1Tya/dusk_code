//! The right column of a chat: its plan and tasks, subagents, changed files, background
//! jobs and what the context holds.

use crate::chat::{data::Agent, subagent::state_text};
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
                // Keyed by what a row shows, so a row is drawn again only when it changes and
                // a finished sun does not replay its hop at every event of another agent.
                {let id = id.clone(); view! {
                    <For each=move || agents.get() key=row_key children=move |agent| {
                        let (parent, thread_id) = (id.clone(), agent.thread_id.clone());
                        view! {
                            <button class="agent" on:click=move |_| app.subagent.set(Some((parent.clone(), thread_id.clone())))>
                                <Sundown state=agent.state.clone() seed=agent.thread_id.clone() />
                                <div class="agent-text">
                                    <div class="agent-name">{agent.name.clone()}<span class="muted">{format!(" · {}", agent.role)}</span></div>
                                    <div class="muted">{agent.task.clone()}</div>
                                    <div class=agent_class(&agent.state)>{state_text(&agent.state)}</div>
                                </div>
                            </button>
                        }
                    } />
                }}
            </Show>
        </div>
    }
}

fn row_key(agent: &Agent) -> (String, String, String, String, String) {
    (
        agent.thread_id.clone(),
        agent.name.clone(),
        agent.role.clone(),
        agent.task.clone(),
        format!("{:?}", agent.state),
    )
}

pub fn agent_class(state: &AgentState) -> &'static str {
    match state {
        AgentState::Failed => "agent-state bad",
        AgentState::Done => "agent-state",
        _ => "agent-state live",
    }
}

/// Sundown, Desktop's mascot (`columns/inspector/mascot.rs`) drawn with CSS: a sun half set
/// on its horizon that moves as the agent's activity does, hops when done and sets when it
/// failed. `seed` shifts its loops so suns side by side do not blink in step.
#[component]
fn Sundown(state: AgentState, seed: String) -> impl IntoView {
    let doing = match &state {
        AgentState::Thinking => "thinking",
        AgentState::Writing => "writing",
        AgentState::Tool(name) => Doing::of_tool(name).label(),
        AgentState::Done => "done",
        AgentState::Failed => "failed",
    };
    let hash = seed.bytes().fold(0u32, |hash, byte| {
        hash.wrapping_mul(31).wrapping_add(byte.into())
    });
    let delay = format!("--d: -{}ms", hash % 4300);
    view! {
        <span class="sun" data-doing=doing style=delay aria-hidden="true">
            <span class="sun-sky"><span class="sun-rise"><span class="sun-disc"><span class="sun-eyes">
                <i class="sun-eye"></i><i class="sun-eye"></i>
            </span></span></span></span>
            <span class="sun-line"></span>
            <span class="sun-glint"></span>
            <span class="sun-glint"></span>
        </span>
    }
}
