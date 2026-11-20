//! Customize: agents, skills, connectors (the MCP servers), plugins, tools and voice.
//! Every tab the store has listings for shows its own Workshop under "Discover".

use crate::{
    api,
    state::{AppState, View, state},
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
    // Discover or Installed as the user chose; until then Discover while nothing is
    // installed on the tab, as in the desktop app.
    let discover = RwSignal::new(None::<bool>);
    let discovering = Signal::derive(move || {
        discover.get().unwrap_or_else(|| {
            tab.scope()
                .is_some_and(|scope| nothing_installed(scope, panel, app))
        })
    });
    view! {
        <div class="page">
            <h2>"Customize"</h2>
            <div class="tabs">
                {TABS.map(|(t, label)| view! {
                    <button class="tab" class:active=move || t == tab
                        on:click=move |_| app.show(View::Customize(t))>{label}</button>
                })}
            </div>
            {tab.scope().map(|_| view! { <workshop::ViewSwitch discover discovering /> })}
            {move || match (tab.scope(), discovering.get()) {
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

/// Whether the settings and status read so far show nothing installed for `scope`; the
/// agents' graph always holds the built-in ones.
fn nothing_installed(scope: Scope, panel: RwSignal<Option<Settings>>, app: AppState) -> bool {
    match scope {
        Scope::Connectors => panel.with(|p| p.as_ref().is_some_and(|p| p.mcp.is_empty())),
        Scope::Skills => panel.with(|p| p.as_ref().is_some_and(|p| p.skills.is_empty())),
        Scope::Plugins => app
            .status
            .with(|s| s.as_ref().is_some_and(|s| s.plugins.is_empty())),
        Scope::Agents => false,
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
