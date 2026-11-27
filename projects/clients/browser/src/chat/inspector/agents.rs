//! The right column of a chat: its plan and tasks, subagents, changed files, background
//! jobs and what the context holds.

use crate::chat::subagent::state_text;
use crate::state::state;
use dusk_protocol::thread::Subagent as Agent;
use dusk_protocol::{agent::Doing, thread::AgentState};
use leptos::prelude::*;

#[component]
pub fn AgentsTab(id: String) -> impl IntoView {
    let app = state();
    let thread = id.clone();
    let agents = Signal::derive(move || {
        app.chat(&thread)
            .map(|c| {
                c.with(|c| {
                    // In the order they started, each with how full its context is.
                    let mut agents: Vec<(Agent, Option<String>)> = c
                        .agents
                        .values()
                        .map(|agent| {
                            let context = c
                                .agent_chats
                                .get(&agent.thread_id)
                                .and_then(|log| log.context_line());
                            (agent.clone(), context)
                        })
                        .collect();
                    agents.sort_by_key(|(agent, _)| agent.started_at);
                    agents
                })
            })
            .unwrap_or_default()
    });
    read_contexts(&id);
    let clear = {
        let thread = id.clone();
        move |_| {
            if let Some(chat) = app.chat(&thread) {
                chat.update(|c| {
                    c.clear_agents(None);
                });
            }
        }
    };
    let finished = move || agents.with(|agents| agents.iter().any(|(agent, _)| !agent.working()));
    view! {
        <div class="agents">
            // Finished agents leave the list for as long as the chat is open on this page;
            // one an approval waits on stays, and work brings one back.
            <Show when=finished>
                <div class="agents-head">
                    <button class="ghost quiet" title="Take the agents that are done off the list" on:click=clear.clone()>"Clear finished"</button>
                </div>
            </Show>
            <Show when=move || !agents.get().is_empty() fallback=|| view! { <p class="muted">"No subagents in this chat."</p> }>
                // Keyed by what a row shows, so a row is drawn again only when it changes and
                // a finished sun does not replay its hop at every event of another agent.
                {let id = id.clone(); view! {
                    <For each=move || agents.get() key=row_key children=move |(agent, context)| {
                        let (parent, thread_id) = (id.clone(), agent.thread_id.clone());
                        view! {
                            <button class="agent" on:click=move |_| app.subagent.set(Some((parent.clone(), thread_id.clone())))>
                                <Sundown state=agent.state.clone() seed=agent.thread_id.clone() />
                                <div class="agent-text">
                                    <div class="agent-name">{agent.name.clone()}<span class="muted">{format!(" · {}", agent.role)}</span></div>
                                    <div class="muted">{agent.task.clone()}</div>
                                    <div class=agent_class(&agent.state)>{state_text(&agent.state)}</div>
                                    {context.map(|context| view! { <div class="agent-context muted">{context}</div> })}
                                </div>
                            </button>
                        }
                    } />
                }}
            </Show>
        </div>
    }
}

type RowKey = (String, String, String, String, String, Option<String>);

fn row_key((agent, context): &(Agent, Option<String>)) -> RowKey {
    (
        agent.thread_id.clone(),
        agent.name.clone(),
        agent.role.clone(),
        agent.task.clone(),
        format!("{:?}", agent.state),
        context.clone(),
    )
}

/// Asks the core how full the context of each subagent of chat `id` is when the page
/// heard nothing of it: an agent that worked before the page opened the chat told no one.
pub fn read_contexts(id: &str) {
    let app = state();
    let Some(chat) = app.chat(id) else {
        return;
    };
    let unknown: Vec<String> = chat.with_untracked(|c| {
        c.agents
            .keys()
            .filter(|agent| {
                c.agent_chats
                    .get(agent.as_str())
                    .is_none_or(|log| log.context_unknown())
            })
            .cloned()
            .collect()
    });
    for agent in unknown {
        leptos::task::spawn_local(async move {
            // The line is an extra: the list stands without it.
            if let Ok(report) = crate::api::context_report(Some(agent.clone())).await {
                chat.update(|c| {
                    c.agent_chats
                        .entry(agent)
                        .or_default()
                        .context_report(&report)
                });
            }
        });
    }
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
