//! The chat page: header, transcript, approvals, queue and composer.

pub mod approval;
mod blocks;
mod composer;
mod pictures;
pub use pictures::Picture;
pub mod data;
pub mod inspector;
pub mod subagent;

use crate::{
    api,
    sidebar::chat_title,
    state::{AppState, Chat as ChatSignal, state},
    ui::{compact, run},
    voice,
};
use blocks::Transcript;
use composer::Composer;
use data::ChatData;
use dusk_protocol::config::ApprovalPolicy;
use leptos::prelude::*;

/// The chat signal of `id`, or a fresh empty one for a draft.
pub fn chat_of(app: AppState, id: Option<&str>) -> ChatSignal {
    id.and_then(|id| app.chat(id))
        .unwrap_or_else(|| RwSignal::new(ChatData::default()))
}

#[component]
pub fn Chat(id: Option<String>) -> impl IntoView {
    let app = state();
    let chat = chat_of(app, id.as_deref());
    let title = {
        let id = id.clone();
        move || match &id {
            None => "New chat".to_owned(),
            Some(id) => chat
                .with(|c| c.title.clone())
                .or_else(|| {
                    app.threads
                        .with(|threads| threads.iter().find(|t| t.id == *id).map(chat_title))
                })
                .unwrap_or_else(|| "Chat".to_owned()),
        }
    };
    let project_name = move || {
        app.project
            .get()
            .and_then(|id| app.project_by_id(&id))
            .map(|p| p.name)
    };
    view! {
        <div class="chat">
            <header class="chat-head">
                <div class="chat-title-line">
                    <h2>{title.clone()}</h2>
                    <span class="muted">{move || project_name().unwrap_or_default()}</span>
                </div>
                <ChatBar id=id.clone() chat />
                {id.clone().map(|id| view! { <ChatMenu id /> })}
            </header>
            <Transcript chat />
            <approval::Approvals chat />
            <approval::AskUser chat id=id.clone() />
            <Composer id chat />
        </div>
    }
}

/// The model, the mode, the access of this chat and the context it uses.
#[component]
fn ChatBar(id: Option<String>, chat: ChatSignal) -> impl IntoView {
    let app = state();
    let model = move || {
        app.status
            .with(|s| s.as_ref().and_then(|s| s.model.clone()))
            .unwrap_or_else(|| "no model".to_owned())
    };
    let mode = move || {
        app.status
            .with(|s| s.as_ref().map(|s| s.mode.clone()))
            .unwrap_or_default()
    };
    let context = move || {
        chat.with(|c| c.tokens)
            .map(|(used, limit)| format!("{} / {}", compact(used), compact(limit)))
    };
    let share = move || {
        chat.with(|c| c.tokens)
            .and_then(|(used, limit)| (used * 100).checked_div(limit))
            .map_or(0, |percent| percent.min(100))
    };
    let access = move || match chat.with(|c| c.access) {
        None => "default",
        Some(ApprovalPolicy::Ask) => "ask",
        Some(ApprovalPolicy::Sandbox) => "sandbox",
        Some(ApprovalPolicy::Arbiter) => "arbiter",
        Some(ApprovalPolicy::FullAccess) => "full_access",
    };
    let set_access = {
        let id = id.clone();
        move |ev| {
            let name = event_target_value(&ev);
            let policy = match name.as_str() {
                "default" => None,
                other => ApprovalPolicy::parse_stored(other).ok(),
            };
            let Some(id) = id.clone() else {
                return app.fail("Send a message first: the access belongs to the chat.");
            };
            run(
                async move { api::set_approvals(&id, policy).await },
                move |()| chat.update(|c| c.access = policy),
            );
        }
    };
    view! {
        <div class="chat-bar">
            <button class="chip" title="Models are chosen in Settings"
                on:click=move |_| app.view.set(crate::state::View::Settings(crate::pages::settings::Page::Model))>
                {model}
            </button>
            <button class="chip" title="Next mode"
                on:click=move |_| run(api::next_mode(), |_| ())>
                {mode}
            </button>
            <select class="chip" title="What this chat may do without asking" prop:value=access on:change=set_access>
                <option value="default">"access: default"</option>
                <option value="ask">"ask"</option>
                <option value="sandbox">"sandbox"</option>
                <option value="arbiter">"arbiter"</option>
                <option value="full_access">"full access"</option>
            </select>
            {move || context().map(|text| view! {
                <span class="context" title="Context used">
                    <span class="meter"><span class="fill" style:width=move || format!("{}%", share())></span></span>
                    {text}
                </span>
            })}
            {move || chat.with(|c| c.cost.clone()).map(|cost| view! { <span class="muted">{cost}</span> })}
            <Show when=move || voice::speaking()><span class="muted">"speaking…"</span></Show>
        </div>
    }
}

/// Rename, close and delete of the chat.
#[component]
fn ChatMenu(id: String) -> impl IntoView {
    let app = state();
    let rename = {
        let id = id.clone();
        move |_| {
            let Some(title) = web_sys::window()
                .and_then(|w| w.prompt_with_message("New title").ok().flatten())
                .filter(|t| !t.trim().is_empty())
            else {
                return;
            };
            let id = id.clone();
            run(async move { api::rename(&id, title).await }, move |()| {
                app.refresh()
            });
        }
    };
    let close = {
        let id = id.clone();
        move |_| app.close_chat(id.clone())
    };
    view! {
        <div class="chat-menu">
            <button class="icon" title="Rename this chat" on:click=rename>"Rename"</button>
            <button class="icon" title="Stop using it here; it stays in the list" on:click=close>"Close"</button>
        </div>
    }
}
