//! A subagent's own chat in a window over the page, with its steps folded as in a chat.

use super::blocks::Transcript;
use crate::{api, state::state, ui::run};
use dusk_protocol::thread::AgentState;
use leptos::prelude::*;

#[component]
pub fn SubagentWindow() -> impl IntoView {
    let app = state();
    move || {
        let (parent, thread_id) = app.subagent.get()?;
        let parent_chat = app.chat(&parent)?;
        let sub = Memo::new({
            let thread_id = thread_id.clone();
            move |_| {
                parent_chat.with(|c| c.agent_chats.get(&thread_id).cloned().unwrap_or_default())
            }
        });
        // The sub-chat as a signal of its own, so the transcript reads it like any chat.
        let view_chat = RwSignal::new(sub.get_untracked());
        Effect::new(move |_| view_chat.set(sub.get()));
        let info = parent_chat
            .with_untracked(|c| c.agents.iter().find(|a| a.thread_id == thread_id).cloned());
        let name = info.as_ref().map(|a| a.name.clone()).unwrap_or_default();
        let task = info.as_ref().map(|a| a.task.clone()).unwrap_or_default();
        let id_for_state = thread_id.clone();
        let stop_id = thread_id.clone();
        Some(view! {
            <div class="overlay" on:click=move |_| app.subagent.set(None)>
                <div class="window" on:click=|ev| ev.stop_propagation()>
                    <header class="window-head">
                        <div>
                            <h3>{name}</h3>
                            <div class="muted">{task}</div>
                        </div>
                        <div class="grow"></div>
                        <span class="chip">{move || {
                            parent_chat.with(|c| c.agents.iter().find(|a| a.thread_id == id_for_state).map(|a| state_text(&a.state)))
                                .unwrap_or_default()
                        }}</span>
                        <button class="ghost danger" on:click=move |_| {
                            let id = stop_id.clone();
                            run(async move { api::stop_agent(&id).await }, |()| ());
                        }>"Stop"</button>
                        <button class="icon" on:click=move |_| app.subagent.set(None)>"×"</button>
                    </header>
                    <Transcript chat=view_chat />
                </div>
            </div>
        })
    }
}

pub fn state_text(state: &AgentState) -> String {
    match state {
        AgentState::Thinking => "thinking".to_owned(),
        AgentState::Writing => "writing".to_owned(),
        AgentState::Tool(name) => format!("using {name}"),
        AgentState::Done => "done".to_owned(),
        AgentState::Failed => "failed".to_owned(),
    }
}
