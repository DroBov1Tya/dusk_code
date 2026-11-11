//! Settings: every page of the desktop app, each a view over the same core settings.

use crate::{
    api,
    state::{View, state},
    ui::{Row, run},
};
use dusk_protocol::{
    client::{SettingsRequest, Switch as Flag},
    config::Settings,
};
use leptos::prelude::*;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Page {
    Appearance,
    Model,
    Chat,
    Safety,
    Secrets,
    AboutMe,
    Notifications,
    VersionControl,
    Memory,
    Advanced,
    Updates,
    Usage,
}

const PAGES: [(Page, &str); 12] = [
    (Page::Appearance, "Appearance"),
    (Page::Model, "Model"),
    (Page::Chat, "Chat"),
    (Page::Safety, "Safety"),
    (Page::Secrets, "Secrets"),
    (Page::AboutMe, "About me"),
    (Page::Notifications, "Notifications"),
    (Page::VersionControl, "Version control"),
    (Page::Memory, "Memory and context"),
    (Page::Advanced, "Advanced"),
    (Page::Updates, "Updates"),
    (Page::Usage, "System usage"),
];

#[component]
pub fn SettingsPage(page: Page) -> impl IntoView {
    let app = state();
    let panel = RwSignal::new(None::<Settings>);
    let reload = move || run(api::settings_panel(), move |p| panel.set(Some(p)));
    reload();
    view! {
        <div class="settings">
            <nav class="settings-nav">
                {PAGES.map(|(p, label)| view! {
                    <button class="nav-item" class:active=move || p == page
                        on:click=move |_| app.view.set(View::Settings(p))>{label}</button>
                })}
            </nav>
            <div class="page">
                {match page {
                    Page::Appearance => view! { <Appearance /> }.into_any(),
                    Page::Model => view! { <ModelPage /> }.into_any(),
                    Page::Chat => view! { <ChatPage panel reload /> }.into_any(),
                    Page::Safety => view! { <Safety panel reload /> }.into_any(),
                    Page::Secrets => view! { <Secrets /> }.into_any(),
                    Page::AboutMe => view! { <AboutMe /> }.into_any(),
                    Page::Notifications => view! { <Notifications panel reload /> }.into_any(),
                    Page::VersionControl => view! { <VersionControl panel reload /> }.into_any(),
                    Page::Memory => view! { <Memory panel reload /> }.into_any(),
                    Page::Advanced => view! { <Advanced panel reload /> }.into_any(),
                    Page::Updates => view! { <Updates /> }.into_any(),
                    Page::Usage => view! { <Usage /> }.into_any(),
                }}
            </div>
        </div>
    }
}

/// Sets a switch in the core, then loads the panel again.
pub fn flip(flag: Flag, on: bool, reload: impl Fn() + 'static) {
    run(api::switch(flag, on), move |()| reload());
}

mod about_me;
mod advanced;
mod appearance;
mod chat;
mod memory;
mod model;
mod notifications;
mod secrets;
mod system;
mod version_control;

use about_me::AboutMe;
use advanced::Advanced;
use appearance::Appearance;
use chat::{ChatPage, Safety};
use memory::Memory;
use model::ModelPage;
use notifications::Notifications;
use secrets::Secrets;
use system::{Updates, Usage};
use version_control::VersionControl;

/// A row with a number to save: the limits and sizes of the memory and advanced pages.
pub fn number_row(
    name: &'static str,
    hint: &'static str,
    value: usize,
    save: impl Fn(usize) -> SettingsRequest + Clone + Send + Sync + 'static,
    reload: impl Fn() + Clone + Send + Sync + 'static,
) -> impl IntoView {
    let text = RwSignal::new(value.to_string());
    view! {
        <Row name hint>
            <input type="number" min="0" prop:value=move || text.get() on:input=move |ev| text.set(event_target_value(&ev)) />
            <button class="ghost" on:click=move |_| {
                let Ok(number) = text.get_untracked().trim().parse::<usize>() else { return };
                let (request, reload) = (save(number), reload.clone());
                run(api::set(request), move |()| reload());
            }>"Save"</button>
        </Row>
    }
}
