//! Settings: every page of the desktop app, each a view over the same core settings.

use crate::{
    state::state,
    ui::{Icon, IconName, Section, theme::THEMES},
};
use leptos::prelude::*;

#[component]
pub fn Appearance() -> impl IntoView {
    let app = state();
    view! {
        <h2>"Appearance"</h2>
        <p class="muted">"The theme is kept in this browser."</p>
        <Section title="Theme">
            <div class="themes">
                {THEMES.iter().map(|name| view! {
                    <button class="theme-card" class:active=move || app.theme.get() == *name
                        aria-pressed=move || if app.theme.get() == *name { "true" } else { "false" }
                        on:click=move |_| app.theme.set((*name).to_owned())>
                        {*name}
                        <Show when=move || app.theme.get() == *name><Icon name=IconName::Check /></Show>
                    </button>
                }).collect_view()}
            </div>
        </Section>
    }
}
