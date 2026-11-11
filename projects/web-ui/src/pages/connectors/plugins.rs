//! Connectors: agents, skills, MCP servers, plugins, tools and voice.

use crate::{
    api,
    state::state,
    ui::{Switch, run},
};
use dusk_protocol::client::SettingsRequest;
use leptos::prelude::*;

#[component]
pub fn Plugins() -> impl IntoView {
    let app = state();
    view! {
        <Show when=move || app.status.with(|s| s.as_ref().is_some_and(|s| !s.plugins.is_empty()))
            fallback=|| view! { <p class="muted">"No plugins are installed."</p> }>
            <div class="cards">{move || app.status.get().map(|s| s.plugins).unwrap_or_default().into_iter().map(|plugin| {
                let toggle = plugin.name.clone();
                view! {
                    <div class="card-tile">
                        <div class="tile-head"><b>{plugin.name.clone()}</b><span class="muted">{plugin.version.clone()}</span></div>
                        <p class="muted">{plugin.description.clone()}</p>
                        {plugin.problem.clone().map(|p| view! { <p class="bad">{p}</p> })}
                        {plugin.missing_settings.clone().into_iter().map(|m| view! { <p class="bad">{format!("Missing setting: {m}")}</p> }).collect_view()}
                        {plugin.secrets.clone().into_iter().map(|(key, set)| {
                            let (name, key_name, value) = (plugin.name.clone(), key.clone(), RwSignal::new(String::new()));
                            view! {
                                <div class="row-actions">
                                    <span>{format!("{key} ({})", if set { "set" } else { "not set" })}</span>
                                    <input type="password" autocomplete="off" prop:value=move || value.get()
                                        on:input=move |ev| value.set(event_target_value(&ev)) />
                                    <button class="ghost" on:click=move |_| {
                                        let request = SettingsRequest::PluginSecret {
                                            plugin: name.clone(), key: key_name.clone(),
                                            value: dusk_protocol::client::Secret(value.get_untracked()),
                                        };
                                        run(api::set(request), move |()| value.set(String::new()));
                                    }>"Save"</button>
                                </div>
                            }
                        }).collect_view()}
                        <Switch on=plugin.enabled label="Enabled" change=move |on| {
                            let request = SettingsRequest::PluginEnabled { name: toggle.clone(), enabled: on };
                            run(api::set(request), |()| ());
                        } />
                    </div>
                }
            }).collect_view()}</div>
        </Show>
    }
}
