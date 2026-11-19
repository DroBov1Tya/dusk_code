//! Customize: agents, skills, connectors (the MCP servers), plugins, tools and voice.
//! Every tab the store has listings for shows its own Workshop under "Discover".

use crate::{
    api,
    state::{View, state},
    ui::run,
};
use dusk_protocol::{config::Settings, plugin::Scope};
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

impl Tab {
    /// The tab of a shelf's Workshop.
    pub fn of(scope: Scope) -> Self {
        match scope {
            Scope::Agents => Self::Agents,
            Scope::Skills => Self::Skills,
            Scope::Connectors => Self::Mcp,
            Scope::Plugins => Self::Plugins,
        }
    }

    /// The Workshop the tab has; Tools and Voice have none in the store.
    fn scope(self) -> Option<Scope> {
        match self {
            Self::Agents => Some(Scope::Agents),
            Self::Skills => Some(Scope::Skills),
            Self::Mcp => Some(Scope::Connectors),
            Self::Plugins => Some(Scope::Plugins),
            Self::Tools | Self::Voice => None,
        }
    }
}

const TABS: [(Tab, &str); 6] = [
    (Tab::Agents, "Agents"),
    (Tab::Skills, "Skills"),
    (Tab::Mcp, "Connectors"),
    (Tab::Plugins, "Plugins"),
    (Tab::Tools, "Tools"),
    (Tab::Voice, "Voice"),
];

#[component]
pub fn Customize(tab: Tab) -> impl IntoView {
    let app = state();
    let panel = RwSignal::new(None::<Settings>);
    let reload = move || run(api::settings_panel(), move |p| panel.set(Some(p)));
    reload();
    let discover = RwSignal::new(false);
    view! {
        <div class="page">
            <h2>"Customize"</h2>
            <div class="tabs">
                {TABS.map(|(t, label)| view! {
                    <button class="tab" class:active=move || t == tab
                        on:click=move |_| app.show(View::Customize(t))>{label}</button>
                })}
            </div>
            {tab.scope().map(|_| view! { <workshop::ViewSwitch discover /> })}
            {move || match (tab.scope(), discover.get()) {
                (Some(scope), true) => view! { <Workshop scope /> }.into_any(),
                _ => match tab {
                    Tab::Agents => view! { <Agents panel reload /> }.into_any(),
                    Tab::Skills => view! { <Skills panel reload discover /> }.into_any(),
                    Tab::Mcp => view! { <Mcp discover /> }.into_any(),
                    Tab::Plugins => view! { <Plugins discover /> }.into_any(),
                    Tab::Tools => view! { <Tools panel reload /> }.into_any(),
                    Tab::Voice => view! { <Voice /> }.into_any(),
                },
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
mod workshop;

pub use workshop::SuggestedCard;

use agents::Agents;
use mcp::Mcp;
use plugins::Plugins;
use skills::Skills;
use tools::Tools;
use voice::Voice;
use workshop::Workshop;
