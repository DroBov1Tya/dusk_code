//! Settings: every page of the desktop app, each a view over the same core settings.

use crate::{
    api,
    ui::{Row, run},
};
use dusk_protocol::client::SettingsRequest;
use leptos::prelude::*;

/// Keys and tokens: whether each is set, never its value.
#[component]
pub fn Secrets() -> impl IntoView {
    let info = RwSignal::new(None::<dusk_protocol::config::SecretsInfo>);
    let load = move || run(api::secrets(), move |s| info.set(Some(s)));
    load();
    view! {
        <h2>"Secrets"</h2>
        <p class="muted">"Keys and tokens, kept in the system keychain. Values are never shown."</p>
        {move || info.get().map(|info| view! {
            {info.problem.clone().map(|p| view! { <p class="bad">{p}</p> })}
            {info.entries.clone().into_iter().map(|entry| {
                let value = RwSignal::new(String::new());
                let (set_name, delete_name) = (entry.name.clone(), entry.name.clone());
                view! {
                    <Row name=entry.label.clone() hint=if entry.set { "set" } else { "not set" }>
                        {entry.editable.then(|| view! {
                            <input type="password" autocomplete="off" placeholder="new value"
                                prop:value=move || value.get() on:input=move |ev| value.set(event_target_value(&ev)) />
                            <button class="ghost" on:click=move |_| {
                                let (name, v) = (set_name.clone(), value.get_untracked());
                                run(api::set_secret(name, v), move |()| { value.set(String::new()); load(); });
                            }>"Save"</button>
                            <button class="ghost danger" on:click=move |_| {
                                let name = delete_name.clone();
                                run(api::set(SettingsRequest::DeleteSecret { name }), move |()| load());
                            }>"Delete"</button>
                        })}
                    </Row>
                }
            }).collect_view()}
        })}
    }
}
