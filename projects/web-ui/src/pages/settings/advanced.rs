//! Settings: every page of the desktop app, each a view over the same core settings.

use crate::ui::{Row, Switch};
use dusk_protocol::{
    client::{SettingsRequest, Switch as Flag},
    config::Settings,
};
use leptos::prelude::*;

use super::{flip, number_row};

#[component]
pub fn Advanced(
    panel: RwSignal<Option<Settings>>,
    reload: impl Fn() + Clone + Send + Sync + 'static,
) -> impl IntoView {
    view! {
        <h2>"Advanced"</h2>
        <p class="muted">"Limits and timings. The defaults suit most work."</p>
        {move || panel.get().map(|p| {
            let (a, b, c, d, e) = (reload.clone(), reload.clone(), reload.clone(), reload.clone(), reload.clone());
            view! {
                {number_row("Agents at work", "How many subagents run at once", p.max_agents_at_work, SettingsRequest::MaxAgentsAtWork, a)}
                {number_row("Goal rounds", "Turns a standing goal may take", p.max_goal_rounds as usize, |n| SettingsRequest::MaxGoalRounds(n as u32), b)}
                {number_row("Idle screen (minutes)", "Before the idle screen of the desktop app", p.idle_screen_minutes as usize, |n| SettingsRequest::IdleScreenMinutes(n as u32), c)}
                <Row name="Logging" hint="Write logs to the logs folder">
                    <Switch on=p.logging change=move |on| flip(Flag::Logging, on, d.clone()) />
                </Row>
                <Row name="Dev channel" hint="Updates from the development channel">
                    <Switch on=p.update_channel == dusk_protocol::release::Channel::Dev
                        change=move |on| flip(Flag::DevChannel, on, e.clone()) />
                </Row>
            }
        })}
    }
}
