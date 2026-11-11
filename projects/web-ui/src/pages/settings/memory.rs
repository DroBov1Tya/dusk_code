//! Settings: every page of the desktop app, each a view over the same core settings.

use crate::{
    api,
    state::state,
    ui::{Row, Switch, run},
};
use dusk_protocol::{
    client::{SettingsRequest, Switch as Flag},
    config::{ContextPolicy, ContextPreset, Settings},
};
use leptos::prelude::*;

use super::{flip, number_row};

#[component]
pub fn Memory(
    panel: RwSignal<Option<Settings>>,
    reload: impl Fn() + Clone + Send + Sync + 'static,
) -> impl IntoView {
    let app = state();
    let _ = app;
    view! {
        <h2>"Memory and context"</h2>
        <p class="muted">"What is remembered between chats, and how much context is kept."</p>
        {move || panel.get().map(|p| {
            let (a, b, c, d) = (reload.clone(), reload.clone(), reload.clone(), reload.clone());
            view! {
                <Row name="Memory" hint="Notes the agents keep about you and your work">
                    <Switch on=p.memory.enabled change=move |on| flip(Flag::Memory, on, a.clone()) />
                </Row>
                {number_row("Memory size", "Characters of memory in each request", p.memory.max_chars, SettingsRequest::MemoryChars, b)}
                <Row name="Work history" hint="A short record of recent work in new chats">
                    <Switch on=p.work_history.enabled change=move |on| flip(Flag::WorkHistory, on, c.clone()) />
                </Row>
                {number_row("Work history size", "Characters of history", p.work_history.max_chars, SettingsRequest::WorkHistoryChars, d)}
                <ContextPicker />
            }
        })}
    }
}

#[component]
pub fn ContextPicker() -> impl IntoView {
    let app = state();
    let current = move || {
        app.status
            .with(|s| s.as_ref().and_then(|s| s.context_policy))
    };
    let pick = move |ev| {
        let value = event_target_value(&ev);
        let policy = match value.as_str() {
            "auto" => ContextPolicy::Auto,
            other => match ContextPreset::ALL.iter().find(|p| p.name() == other) {
                Some(preset) => ContextPolicy::Fixed(*preset),
                None => return,
            },
        };
        run(api::number(SettingsRequest::ContextPolicy(policy)), |_| ());
    };
    let selected = move || match current() {
        Some(ContextPolicy::Fixed(p)) => p.name().to_owned(),
        Some(ContextPolicy::Manual(n)) => format!("manual {n}"),
        _ => "auto".to_owned(),
    };
    view! {
        <Row name="Context size" hint="The largest request to the model">
            <select prop:value=selected on:change=pick>
                <option value="auto">"Auto (the model's window)"</option>
                {ContextPreset::ALL.map(|p| view! { <option value=p.name()>{p.name()}</option> })}
            </select>
        </Row>
    }
}
