//! The chat page: header, transcript, approvals, queue and composer.

pub mod approval;
mod blocks;
mod composer;
mod controls;
mod markdown;
mod pictures;
mod voice;
pub use composer::Draft;
pub use pictures::Picture;
pub mod data;
pub mod inspector;
pub mod subagent;

use crate::{
    api,
    state::{AppState, Chat as ChatSignal, state},
    ui::{Icon, IconName, run},
};
use blocks::Transcript;
use composer::Composer;
use data::ChatData;
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
    let open = id.is_some();
    let menu_id = id.clone();
    let project_name = move || {
        app.project
            .get()
            .and_then(|id| app.project_by_id(&id))
            .map(|p| p.name)
    };
    view! {
        <div class="chat">
            <Show when=move || open || project_name().is_some()>
                <header class="chat-head">
                    <span class="chat-project muted">{move || project_name().unwrap_or_default()}</span>
                    {menu_id.clone().map(|id| view! { <ChatMenu id /> })}
                </header>
            </Show>
            <Transcript chat />
            <approval::Approvals chat />
            <approval::AskUser chat id=id.clone() />
            <Composer id chat />
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
            <button class="ghost" title="Rename this chat" on:click=rename>
                <Icon name=IconName::FileText />"Rename"
            </button>
            <button class="ghost" title="Stop using it here; it stays in the list" on:click=close>
                <Icon name=IconName::Close />"Close"
            </button>
        </div>
    }
}
