//! The Model page: connect a provider with a name, an address and a token. The core
//! checks the server first, since only the check can say it is unreachable or refuses the
//! token, then saves it and picks its first model itself while none is chosen.

use super::Welcome;
use crate::{api, state::state};
use dusk_protocol::{
    client::{ProviderTarget, Response, Secret, SettingsRequest},
    config::ProviderKind,
    model::CheckFailure,
};
use leptos::{prelude::*, task::spawn_local};

/// Why the last attempt failed: the kind, when the core could tell it, and its words.
#[derive(Clone)]
struct Failed {
    cause: Option<CheckFailure>,
    said: String,
}

#[component]
pub fn Provider(welcome: Welcome) -> impl IntoView {
    view! {
        <div class="welcome-page form">
            <h2>"Connect a model"</h2>
            <p class="muted">"Your own provider: any server with the OpenAI API, in the cloud or on this machine."</p>
            {move || match welcome.connected.get() {
                Some(name) => view! { <Connected name /> }.into_any(),
                None => view! { <Form welcome /> }.into_any(),
            }}
        </div>
    }
}

/// The provider is saved; the model is the one the core picked, once its status says so.
#[component]
fn Connected(name: String) -> impl IntoView {
    let model = move || {
        state()
            .status
            .with(|s| s.as_ref().and_then(|s| s.model.clone()))
    };
    view! {
        <div class="welcome-connected" role="status">
            <span class="welcome-tick" aria-hidden="true">"\u{2713}"</span>
            <div>
                <b>{format!("Connected to {name}")}</b>
                <p class="muted small">{move || match model() {
                    Some(model) => format!("Chats will use {model}. Change it any time from the model menu."),
                    None => "Choosing its first model\u{2026}".to_owned(),
                }}</p>
            </div>
        </div>
    }
}

#[component]
fn Form(welcome: Welcome) -> impl IntoView {
    let token = RwSignal::new(String::new());
    let (name, address) = (welcome.name, welcome.address);
    let busy = RwSignal::new(false);
    let error = RwSignal::new(None::<Failed>);
    let connect = move || {
        if busy.get_untracked() {
            return;
        }
        let base_url = address.get_untracked().trim().to_owned();
        if base_url.is_empty() {
            let said = "Type the server's address first.".into();
            return error.set(Some(Failed { cause: None, said }));
        }
        // The token leaves the box at once and goes to the core only.
        let api_key = Some(token.get_untracked())
            .filter(|t| !t.is_empty())
            .map(Secret);
        token.set(String::new());
        let name = Some(name.get_untracked().trim().to_owned()).filter(|n| !n.is_empty());
        busy.set(true);
        error.set(None);
        spawn_local(async move {
            let result = connect_provider(name, base_url, api_key).await;
            busy.set(false);
            match result {
                Ok(saved) => welcome.connected.set(Some(saved)),
                Err(failed) => error.set(Some(failed)),
            }
        });
    };
    let on_key = move |ev: leptos::ev::KeyboardEvent| {
        if ev.key() == "Enter" {
            connect();
        }
    };
    view! {
        <div class="welcome-fields" on:keydown=on_key>
            <label class="field">
                <span>"Name"<small class="muted">" how the model menu will call it"</small></span>
                <input placeholder="Such as my-server" prop:value=move || name.get()
                    on:input=move |ev| name.set(event_target_value(&ev)) />
            </label>
            <label class="field">
                <span>"Address"<small class="muted">" a server with the OpenAI API"</small></span>
                <input placeholder="https://api.example.com/v1 or http://127.0.0.1:11434/v1"
                    prop:value=move || address.get() on:input=move |ev| address.set(event_target_value(&ev)) />
            </label>
            <label class="field">
                <span>"Token"<small class="muted">" kept in the system keychain"</small></span>
                <input type="password" autocomplete="off" placeholder="Empty for a local server"
                    prop:value=move || token.get() on:input=move |ev| token.set(event_target_value(&ev)) />
            </label>
            {move || error.get().map(failure)}
            <button class="primary welcome-connect-button" disabled=move || busy.get() on:click=move |_| connect()>
                {move || if busy.get() { "Connecting\u{2026}" } else { "Connect" }}
            </button>
        </div>
    }
}

/// Checks the server, then saves it; the answer is the name it was saved under.
async fn connect_provider(
    name: Option<String>,
    base_url: String,
    api_key: Option<Secret>,
) -> Result<String, Failed> {
    let plain = |said: String| Failed { cause: None, said };
    let kind = ProviderKind::OpenAiCompatible;
    let target = ProviderTarget::New {
        kind,
        base_url: base_url.clone(),
        api_key: api_key.clone(),
    };
    let status = match crate::link::request(dusk_protocol::client::Request::Settings(
        SettingsRequest::CheckProvider(target),
    ))
    .await
    .map_err(plain)?
    {
        Response::Provider(status) => status,
        other => {
            return Err(plain(format!(
                "The core gave an unexpected answer: {other:?}"
            )));
        }
    };
    if let Err(said) = status.models {
        let cause = status.failure;
        return Err(Failed { cause, said });
    }
    let saved = api::connect_provider(SettingsRequest::Connect {
        kind,
        name,
        base_url,
        api_key,
    })
    .await
    .map_err(plain)?;
    state().refresh();
    Ok(saved)
}

/// One plain line for the kind of failure, then the core's own words, quieter.
fn failure(failed: Failed) -> impl IntoView {
    let headline = match failed.cause {
        Some(CheckFailure::Unreachable) => Some("Cannot reach this address."),
        Some(CheckFailure::Refused) => Some("The server refused the token."),
        Some(CheckFailure::NotCompatible) => {
            Some("The server did not answer like an OpenAI-compatible API.")
        }
        None => None,
    };
    view! {
        <div class="welcome-error" role="alert">
            {headline.map(|line| view! { <p class="error">{line}</p> })}
            <p class=if headline.is_some() { "muted small" } else { "error" }>{failed.said}</p>
        </div>
    }
}
