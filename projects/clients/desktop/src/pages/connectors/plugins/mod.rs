//! The plugins shelf: the installed plugins as cards, with what each brings and whether it
//! waits for settings, or the Workshop, where the store's plugins, skills, packs and
//! agents are installed.

mod confirm;
pub mod workshop;

pub use confirm::{Confirm, render as confirm};
pub use workshop::{Scope, View, Workshop};

use gpui_kit::{
    component::{ActiveTheme, Sizable, switch::Switch},
    *,
};

use super::{card, grid, matches};
use crate::workspace::Workspace;

pub fn render(ws: &Workspace, query: &str, window: &Window, cx: &mut Context<Workspace>) -> Div {
    let body = match ws.view(Scope::Plugins) {
        View::Discover => workshop::render(ws, query, window, cx),
        View::Installed => installed(ws, query, cx),
    };
    div()
        .flex()
        .flex_col()
        .gap_4()
        .child(workshop::switch(ws, Scope::Plugins, cx))
        .child(body)
}

fn installed(ws: &Workspace, query: &str, cx: &mut Context<Workspace>) -> Div {
    let warning = cx.theme().warning;
    let plugins: Vec<_> = ws
        .status
        .plugins
        .iter()
        .filter(|plugin| matches(query, &[&plugin.name, &plugin.description]))
        .cloned()
        .collect();
    if plugins.is_empty() {
        return div().child(crate::ui::empty(
            gpui_kit::component::IconName::Network,
            "No plugins installed",
            "Plugins bring skills, agents and tools; find one in the Workshop or put one in rules/plugins/.",
            cx,
        ));
    }
    let mut cards = grid(cx);
    for plugin in plugins {
        let name = plugin.name.clone();
        let open = plugin.name.clone();
        let state = (!plugin.missing_settings.is_empty()).then(|| {
            (
                format!("Needs {}", plugin.missing_settings.join(", ")),
                warning,
            )
        });
        cards = cards.child(
            card::render(
                card::Card {
                    id: SharedString::from(format!("plugin-card-{name}")),
                    name: plugin.name.clone(),
                    kind: format!("Plugin {} · {}", plugin.version, plugin.parts.join(", ")),
                    description: plugin.description.clone(),
                    state,
                    icon: None,
                },
                Switch::new(SharedString::from(format!("plugin-{name}")))
                    .checked(plugin.enabled)
                    .small()
                    .on_click(
                        cx.listener(move |ws, on: &bool, _, cx| ws.plugin_enabled(&name, *on, cx)),
                    ),
                cx,
            )
            .on_click(cx.listener(move |ws, _, window, cx| ws.open_plugin(&open, window, cx))),
        );
    }
    cards
}

impl Workspace {
    fn plugin_enabled(&mut self, name: &str, on: bool, cx: &mut Context<Self>) {
        let name = name.to_owned();
        self.core.call(
            cx,
            move |client| async move { client.settings().plugin_enabled(&name, on).await },
            |ws, result, cx| {
                if let Err(error) = result {
                    ws.fail(error, cx);
                }
            },
        );
    }
}
