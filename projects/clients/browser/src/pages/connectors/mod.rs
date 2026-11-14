//! Connectors: agents, skills, MCP servers, plugins, tools and voice.

use crate::{
    api,
    state::{View, state},
    ui::run,
};
use dusk_protocol::config::Settings;
use leptos::prelude::*;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tab {
    Agents,
    Skills,
    Mcp,
    Plugins,
    Tools,
    Voice,
}

const TABS: [(Tab, &str); 6] = [
    (Tab::Agents, "Agents"),
    (Tab::Skills, "Skills"),
    (Tab::Mcp, "MCP"),
    (Tab::Plugins, "Plugins"),
    (Tab::Tools, "Tools"),
    (Tab::Voice, "Voice"),
];

#[component]
pub fn Connectors(tab: Tab) -> impl IntoView {
    let app = state();
    let panel = RwSignal::new(None::<Settings>);
    let reload = move || run(api::settings_panel(), move |p| panel.set(Some(p)));
    reload();
    view! {
        <div class="page">
            <h2>"Connectors"</h2>
            <div class="tabs">
                {TABS.map(|(t, label)| view! {
                    <button class="tab" class:active=move || t == tab
                        on:click=move |_| app.view.set(View::Connectors(t))>{label}</button>
                })}
            </div>
            {match tab {
                Tab::Agents => view! { <Agents panel reload /> }.into_any(),
                Tab::Skills => view! { <Skills panel reload /> }.into_any(),
                Tab::Mcp => view! { <Mcp /> }.into_any(),
                Tab::Plugins => view! { <Plugins /> }.into_any(),
                Tab::Tools => view! { <Tools panel reload /> }.into_any(),
                Tab::Voice => view! { <Voice /> }.into_any(),
            }}
        </div>
    }
}

/// A text file of the catalog open for editing.
mod agents;
mod editor;
mod mcp;
mod plugins;
mod skills;
mod tools;
mod voice;

use agents::Agents;
use mcp::Mcp;
use plugins::Plugins;
use skills::Skills;
use tools::Tools;
use voice::Voice;
