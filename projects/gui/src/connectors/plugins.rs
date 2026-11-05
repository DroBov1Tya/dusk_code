//! Plugins as cards: what each brings and whether it waits for settings.

use gpui_kit::{
    component::{ActiveTheme, Sizable, switch::Switch},
    *,
};

use super::{card, grid, matches};
use crate::{modal::detail::Detail, widgets, workspace::Workspace};

pub fn render(ws: &Workspace, query: &str, cx: &mut Context<Workspace>) -> Div {
    let warning = cx.theme().warning;
    let plugins: Vec<_> = ws
        .status
        .plugins
        .iter()
        .filter(|plugin| matches(query, &[&plugin.name, &plugin.description]))
        .cloned()
        .collect();
    if plugins.is_empty() {
        return div().child(widgets::muted("No plugins installed.", cx));
    }
    let mut cards = grid();
    for plugin in plugins {
        let name = plugin.name.clone();
        let open = plugin.name.clone();
        let state = (!plugin.missing_settings.is_empty()).then(|| {
            (format!("Needs {}", plugin.missing_settings.join(", ")), warning)
        });
        cards = cards.child(card::render(
            card::Card {
                id: SharedString::from(format!("plugin-card-{name}")),
                name: plugin.name.clone(),
                kind: format!("Plugin {} · {}", plugin.version, plugin.parts.join(", ")),
                description: plugin.description.clone(),
                state,
            },
            Switch::new(SharedString::from(format!("plugin-{name}")))
                .checked(plugin.enabled)
                .small()
                .on_click(cx.listener(move |ws, on: &bool, _, cx| ws.plugin_enabled(&name, *on, cx))),
            cx,
        )
        .on_click(cx.listener(move |ws, _, _, cx| ws.open_detail(Detail::Plugin(open.clone()), cx))));
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
