//! The first screen of a clean install, as the desktop app has it: while the core has no
//! provider, the chat page offers to connect one with a name, an address and a token. The
//! core checks the server first, since only the check can say it is unreachable or
//! refuses the token, then saves it and the first model it gave is chosen.

use crate::{api, chat::Chat, state::state};
use dusk_protocol::{
    client::{Answer, Command, ProviderTarget, Response, Secret, SettingsRequest},
    config::ProviderKind,
    model::CheckFailure,
};
use leptos::{prelude::*, task::spawn_local};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// Not shown: providers are known, the core has not answered yet, or "Not now".
    Hidden,
    Intro,
    Form,
}

/// Why the last attempt failed: the kind, when the core could tell it, and its words.
#[derive(Clone)]
struct Failed {
    cause: Option<CheckFailure>,
    said: String,
}

/// Asks the core for its providers once the link is up (the first status says so); none
/// means a clean install.
pub fn ask(step: RwSignal<Step>) {
    let asked = StoredValue::new(false);
    Effect::new(move |_| {
        if state().status.with(Option::is_none) || asked.get_value() {
            return;
        }
        asked.set_value(true);
        spawn_local(async move {
            if let Ok(Answer::Providers { providers, .. }) =
                api::command(Command::Provider { name: None }).await
                && providers.is_empty()
            {
                step.set(Step::Intro);
            }
        });
    });
}

/// The chat page, or the welcome in its place.
#[component]
pub fn ChatOrWelcome(id: Option<String>, step: RwSignal<Step>) -> impl IntoView {
    move || {
        match step.get() {
        Step::Hidden => view! { <Chat id=id.clone() /> }.into_any(),
        Step::Intro => view! {
            <div class="welcome">
                <img src="mark.png" alt="" width="56" height="56" />
                <h1>"duskcode"</h1>
                <p class="muted">"An agent that works in your projects, on the model you choose."</p>
                <button class="welcome-connect" on:click=move |_| step.set(Step::Form)>
                    <b>"I want to connect my own AI"</b>
                    <span>"your own provider"</span>
                </button>
                <button class="link quiet" on:click=move |_| step.set(Step::Hidden)>"Not now"</button>
            </div>
        }
        .into_any(),
        Step::Form => view! { <Form step /> }.into_any(),
    }
    }
}

#[component]
fn Form(step: RwSignal<Step>) -> impl IntoView {
    let (name, address, token) = (
        RwSignal::new(String::new()),
        RwSignal::new(String::new()),
        RwSignal::new(String::new()),
    );
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
                Ok(()) => step.set(Step::Hidden),
                Err(failed) => error.set(Some(failed)),
            }
        });
    };
    let on_key = move |ev: leptos::ev::KeyboardEvent| match ev.key().as_str() {
        "Enter" => connect(),
        "Escape" => step.set(Step::Intro),
        _ => {}
    };
    view! {
        <div class="welcome form" on:keydown=on_key>
            <h2>"Your own provider"</h2>
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
            <div class="welcome-actions">
                <button class="link quiet" on:click=move |_| { error.set(None); step.set(Step::Intro) }>"Back"</button>
                <button class="primary" disabled=move || busy.get() on:click=move |_| connect()>
                    {move || if busy.get() { "Connecting\u{2026}" } else { "Connect" }}
                </button>
            </div>
        </div>
    }
}

/// Checks the server, saves it, then chooses the first model it gave.
async fn connect_provider(
    name: Option<String>,
    base_url: String,
    api_key: Option<Secret>,
) -> Result<(), Failed> {
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
    let first = match status.models {
        Ok(models) => models.into_iter().next().map(|model| model.id),
        Err(said) => {
            let cause = status.failure;
            return Err(Failed { cause, said });
        }
    };
    let saved = api::connect_provider(SettingsRequest::Connect {
        kind,
        name,
        base_url,
        api_key,
    })
    .await
    .map_err(plain)?;
    if let Some(model) = first {
        let name = Some(format!("{saved}/{model}"));
        api::command(Command::Model { name }).await.map_err(plain)?;
    }
    state().refresh();
    Ok(())
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
