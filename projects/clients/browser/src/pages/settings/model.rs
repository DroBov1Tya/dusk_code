//! Settings: every page of the desktop app, each a view over the same core settings.

use crate::{
    api,
    chat::data::now_ms,
    state::state,
    ui::{Section, Switch, confirm, run},
};
use dusk_protocol::{
    client::{Answer, Command, SettingsRequest},
    config::ProviderKind,
    model::Effort,
};
use leptos::prelude::*;

use super::model_tiers::ModelTiers;

#[component]
pub fn ModelPage() -> impl IntoView {
    let app = state();
    // Opened from a chat's model button, the page chooses for that chat; each chat keeps
    // its own, and new chats start with the last chosen.
    let chat_id = StoredValue::new(app.model_chat.get_untracked());
    app.model_chat.set(None);
    let chat = chat_id.get_value().and_then(|id| app.chat(&id));
    let providers = RwSignal::new(Vec::new());
    let current = RwSignal::new(None::<String>);
    let busy = RwSignal::new(false);
    // Filters the models below by model or provider name.
    let query = RwSignal::new(String::new());
    let load = move || {
        busy.set(true);
        let thread_id = chat_id.get_value();
        run(
            api::command(Command::Model {
                name: None,
                thread_id,
            }),
            move |answer| {
                busy.set(false);
                if let Answer::Providers {
                    current: now,
                    providers: list,
                } = answer
                {
                    current.set(now);
                    providers.set(list);
                }
            },
        );
    };
    load();
    let effort = move || match chat.filter(|chat| chat.with(|c| c.model.is_some())) {
        Some(chat) => chat.with(|c| c.effort),
        None => app.status.with(|s| s.as_ref().and_then(|s| s.effort)),
    };
    let scope = if chat.is_some() {
        "For this chat and the chats started from now on; other chats keep their own."
    } else {
        "For the chats started from now on; each chat keeps its own."
    };
    view! {
        <h2>"Model"</h2>
        <p class="muted">"The models the chat can use, and the servers that give them."</p>
        <p class="muted">{scope}</p>
        <Section title="Reasoning effort">
            <select prop:value=move || effort().map(|e| e.name()).unwrap_or("default") on:change=move |ev| {
                let level = Some(event_target_value(&ev));
                let thread_id = chat_id.get_value();
                run(api::command(Command::Effort { level, thread_id }), |_| ());
            }>
                {Effort::ALL.map(|e| view! { <option value=e.name()>{e.name()}</option> })}
            </select>
        </Section>
        <Section title="Providers">
            <Show when=move || busy.get()><p class="muted">"Checking the providers…"</p></Show>
            <input type="search" placeholder="Find a model" aria-label="Find a model"
                prop:value=move || query.get() on:input=move |ev| query.set(event_target_value(&ev)) />
            {move || {
                let wanted = query.get().trim().to_lowercase();
                providers.get().into_iter().filter_map(|mut provider| {
                    // A provider's name matching keeps all its models; otherwise the matching ones.
                    if !wanted.is_empty() && !provider.name.to_lowercase().contains(&wanted) {
                        let models = provider.models.as_mut().ok()?;
                        models.retain(|model| model.id.to_lowercase().contains(&wanted));
                        if models.is_empty() {
                            return None;
                        }
                    }
                    Some(provider)
                }).collect::<Vec<_>>()
            }.into_iter().map(|provider| {
                let name = provider.name.clone();
                let removing = name.clone();
                let switched = name.clone();
                let enabled = provider.enabled;
                let named = name.clone();
                let health = move || app.status.with(|status| {
                    let found = status.as_ref()?.provider_health.iter().find(|h| h.name == named)?;
                    Some(found.calls.text(now_ms()))
                });
                view! {
                    <div class="provider" style:opacity=if enabled { "1" } else { "0.5" }>
                        <div class="provider-head">
                            <b>{name.clone()}</b>
                            {provider.balance.as_ref().and_then(|b| b.as_ref().ok()).map(|b| view! { <span class="muted">{b.text()}</span> })}
                            {move || health().map(|text| view! { <span class="muted">{text}</span> })}
                            <span class="grow"></span>
                            <button class="ghost" on:click=move |_| {
                                let name = switched.clone();
                                run(api::set(SettingsRequest::ProviderEnabled { name, enabled: !enabled }), move |()| load());
                            }>{if enabled { "Turn off" } else { "Turn on" }}</button>
                            <button class="ghost danger" on:click=move |_| {
                                if confirm("Remove this provider and its key?") {
                                    let name = removing.clone();
                                    run(api::set(SettingsRequest::RemoveProvider { name }), move |()| load());
                                }
                            }>"Remove"</button>
                        </div>
                        {provider.limits.as_ref().filter(|_| enabled).map(|limits| view! { <p class="muted">{format!("Limits: {}", limits.text())}</p> })}
                        {match &provider.models {
                            _ if !enabled => view! { <p class="muted">"Off: not asked, its key is kept"</p> }.into_any(),
                            Err(error) => view! { <p class="bad">{error.clone()}</p> }.into_any(),
                            Ok(models) => models.iter().map(|model| {
                                let full = format!("{}/{}", provider.name, model.id);
                                let active = current.get_untracked().as_deref() == Some(full.as_str());
                                let pick = full.clone();
                                let shown = !provider.is_hidden(&model.id);
                                let (owner, id) = (provider.name.clone(), model.id.clone());
                                view! {
                                    <div class="model-row" class:hidden=!shown>
                                        <button class="model" class:active=active on:click=move |_| {
                                            let pick = pick.clone();
                                            let thread_id = chat_id.get_value();
                                            run(api::command(Command::Model { name: Some(pick), thread_id }), move |_| load());
                                        }>
                                            <span>{model.id.clone()}</span>
                                            <span class="muted">{model.context_window.map(crate::ui::compact).unwrap_or_default()}</span>
                                        </button>
                                        <span title=if shown { "Offered in the model menus" } else { "Hidden from the model menus" }>
                                            <Switch on=shown change=move |on: bool| {
                                                let (owner, id) = (owner.clone(), id.clone());
                                                let request = SettingsRequest::ModelsHidden {
                                                    provider: owner.clone(),
                                                    models: vec![id.clone()],
                                                    hidden: !on,
                                                };
                                                run(api::set(request), move |()| providers.update(|list| {
                                                    if let Some(status) = list.iter_mut().find(|p| p.name == owner) {
                                                        status.hidden.retain(|hidden| *hidden != id);
                                                        if !on {
                                                            status.hidden.push(id.clone());
                                                        }
                                                    }
                                                }));
                                            } />
                                        </span>
                                    </div>
                                }
                            }).collect_view().into_any(),
                        }}
                    </div>
                }
            }).collect_view()}
        </Section>
        <ModelTiers providers />
        <ConnectProvider done=load />
    }
}

#[component]
pub fn ConnectProvider(done: impl Fn() + Clone + Send + Sync + 'static) -> impl IntoView {
    let kind = RwSignal::new(ProviderKind::OpenAiCompatible);
    let (name, base, key) = (
        RwSignal::new(String::new()),
        RwSignal::new(String::new()),
        RwSignal::new(String::new()),
    );
    let connect = move |_| {
        let api_key = Some(key.get_untracked())
            .filter(|k| !k.is_empty())
            .map(dusk_protocol::client::Secret);
        let request = SettingsRequest::Connect {
            kind: kind.get_untracked(),
            name: Some(name.get_untracked()).filter(|n| !n.is_empty()),
            base_url: base.get_untracked(),
            api_key,
        };
        let done = done.clone();
        run(api::connect_provider(request), move |_| {
            key.set(String::new());
            done();
        });
    };
    view! {
        <Section title="Connect a provider">
            <select on:change=move |ev| kind.set(match event_target_value(&ev).as_str() {
                "openai" => ProviderKind::OpenAi,
                "anthropic" => ProviderKind::Anthropic,
                "google" => ProviderKind::Google,
                _ => ProviderKind::OpenAiCompatible,
            })>
                <option value="openai_compatible">"OpenAI-compatible"</option>
                <option value="openai">"OpenAI"</option>
                <option value="anthropic">"Anthropic"</option>
                <option value="google">"Google AI"</option>
            </select>
            <input placeholder="Name (optional)" prop:value=move || name.get() on:input=move |ev| name.set(event_target_value(&ev)) />
            <input placeholder=move || kind.get().base_url().unwrap_or("Base URL, such as http://localhost:11434/v1")
                prop:value=move || base.get() on:input=move |ev| base.set(event_target_value(&ev)) />
            <input type="password" autocomplete="off" placeholder="API key (stays in the keychain)"
                prop:value=move || key.get() on:input=move |ev| key.set(event_target_value(&ev)) />
            <button class="primary" on:click=connect>"Connect"</button>
        </Section>
    }
}
