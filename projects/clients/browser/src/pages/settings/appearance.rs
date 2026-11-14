//! Settings: every page of the desktop app, each a view over the same core settings.

use crate::{state::state, theme::THEMES, ui::Section};
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
                        on:click=move |_| app.theme.set((*name).to_owned())>{*name}</button>
                }).collect_view()}
            </div>
        </Section>
    }
}
