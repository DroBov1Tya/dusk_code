//! Settings: every page of the desktop app, each a view over the same core settings.

use crate::{
    api,
    ui::{Row, run},
};
use dusk_protocol::{client::SettingsRequest, config::Settings};
use leptos::prelude::*;

#[component]
pub fn VersionControl(
    panel: RwSignal<Option<Settings>>,
    reload: impl Fn() + Clone + Send + Sync + 'static,
) -> impl IntoView {
    view! {
        <h2>"Version control"</h2>
        <p class="muted">"Branches the agents create, and how they are named."</p>
        {move || panel.get().map(|p| {
            let reload = reload.clone();
            let prefix = RwSignal::new(p.version_control.branch_prefix.clone());
            view! {
                <Row name="Branch prefix" hint="Such as dusk/ ; empty adds none">
                    <input prop:value=move || prefix.get() on:input=move |ev| prefix.set(event_target_value(&ev)) />
                    <button class="ghost" on:click=move |_| {
                        let (value, reload) = (prefix.get_untracked(), reload.clone());
                        run(api::set(SettingsRequest::BranchPrefix(value)), move |()| reload());
                    }>"Save"</button>
                </Row>
            }
        })}
    }
}
