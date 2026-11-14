//! The controls around the message box, as the desktop app has them: the access inside
//! the box, the mode and the model under it, the context ring beside it.

use crate::{
    api,
    state::{Chat as ChatSignal, View, state},
    ui::{compact, run},
};
use dusk_protocol::config::ApprovalPolicy;
use leptos::prelude::*;

/// What this chat's tools may do without asking; it belongs to the chat once it exists.
#[component]
pub fn Access(id: Option<String>, chat: ChatSignal) -> impl IntoView {
    let app = state();
    let access = move || match chat.with(|c| c.access) {
        None => "default",
        Some(ApprovalPolicy::Ask) => "ask",
        Some(ApprovalPolicy::Sandbox) => "sandbox",
        Some(ApprovalPolicy::Arbiter) => "arbiter",
        Some(ApprovalPolicy::FullAccess) => "full_access",
    };
    let set_access = move |ev| {
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
    };
    view! {
        <select class="chip quiet" title="What this chat may do without asking" prop:value=access on:change=set_access>
            <option value="default">"default access"</option>
            <option value="ask">"ask"</option>
            <option value="sandbox">"sandbox"</option>
            <option value="arbiter">"arbiter"</option>
            <option value="full_access">"full access"</option>
        </select>
    }
}

/// The mode, under the box on the left.
#[component]
pub fn Mode() -> impl IntoView {
    let app = state();
    let mode = move || {
        app.status
            .with(|s| {
                s.as_ref()
                    .map(|s| format!("{} mode", s.mode.to_lowercase()))
            })
            .unwrap_or_default()
    };
    view! {
        <button class="under" title="Next mode" on:click=move |_| run(api::next_mode(), |_| ())>
            {mode}<span class="caret"></span>
        </button>
    }
}

/// The model, under the box on the right, its name cut short.
#[component]
pub fn Model() -> impl IntoView {
    let app = state();
    let full = move || {
        app.status
            .with(|s| s.as_ref().and_then(|s| s.model.clone()))
            .unwrap_or_else(|| "no model".to_owned())
    };
    let short = move || {
        let full = full();
        let name = full.rsplit('/').next().unwrap_or(&full).to_owned();
        match name.char_indices().nth(22) {
            Some((end, _)) => format!("{}\u{2026}", &name[..end]),
            None => name,
        }
    };
    view! {
        <button class="under" title=full
            on:click=move |_| app.view.set(View::Settings(crate::pages::settings::Page::Model))>
            {short}<span class="caret"></span>
        </button>
    }
}

/// How full the context is: a ring with the share in its middle; the counts and the cost
/// on hover.
#[component]
pub fn Context(chat: ChatSignal) -> impl IntoView {
    let share = move || {
        chat.with(|c| c.tokens)
            .and_then(|(used, limit)| (used * 100).checked_div(limit))
            .map_or(0, |percent| percent.min(100))
    };
    let tip = move || {
        let counts = chat
            .with(|c| c.tokens)
            .map(|(used, limit)| format!("{} of {} tokens", compact(used), compact(limit)))
            .unwrap_or_else(|| "no context yet".to_owned());
        match chat.with(|c| c.cost.clone()) {
            Some(cost) => format!("{counts} \u{00B7} {cost}"),
            None => counts,
        }
    };
    let tone = move || match share() {
        91.. => "ring bad",
        71..=90 => "ring warn",
        _ => "ring",
    };
    view! {
        <div class=tone title=tip style:--share=move || share().to_string()>
            <span>{share}</span>
        </div>
    }
}
