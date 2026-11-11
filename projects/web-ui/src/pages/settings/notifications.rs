//! Settings: every page of the desktop app, each a view over the same core settings.

use crate::ui::{Row, Switch};
use dusk_protocol::{client::Switch as Flag, config::Settings};
use leptos::prelude::*;

use super::flip;

#[component]
pub fn Notifications(
    panel: RwSignal<Option<Settings>>,
    reload: impl Fn() + Clone + Send + Sync + 'static,
) -> impl IntoView {
    view! {
        <h2>"Notifications"</h2>
        <p class="muted">"How you hear that a chat needs you. These apply to the desktop app and the terminal."</p>
        {move || panel.get().map(|p| {
            let (a, b) = (reload.clone(), reload.clone());
            view! {
                <Row name="Notifications" hint="A system notice when a chat needs you">
                    <Switch on=p.notifications change=move |on| flip(Flag::Notifications, on, a.clone()) />
                </Row>
                <Row name="Sounds" hint="A short sound with it">
                    <Switch on=p.sound_notifications change=move |on| flip(Flag::SoundNotifications, on, b.clone()) />
                </Row>
            }
        })}
    }
}
