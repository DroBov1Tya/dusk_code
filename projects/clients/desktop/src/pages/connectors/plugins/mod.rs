//! The plugins shelf: the installed plugins as cards, with what each brings and whether it
//! waits for settings, or the Workshop, where the store's plugins are installed.

mod confirm;
mod listing;
mod workshop;

pub use confirm::{Confirm, render as confirm};
pub use workshop::Workshop;

use gpui_kit::{
    component::{
        ActiveTheme, Icon, Sizable,
        button::{Button, ButtonVariants},
        switch::Switch,
    },
    prelude::FluentBuilder,
    *,
};

use super::{card, grid, matches};
use crate::{ui::controls::choice, workspace::Workspace};

pub fn render(ws: &Workspace, query: &str, cx: &mut Context<Workspace>) -> Div {
    let body = if ws.workshop.open {
        workshop::render(ws, query, cx)
    } else {
        installed(ws, query, cx)
    };
    div()
        .flex()
        .flex_col()
        .gap_4()
        .child(views(ws, cx))
        .child(body)
}

/// "Installed | Workshop", and a refresh of the store while it is shown.
fn views(ws: &Workspace, cx: &mut Context<Workspace>) -> Div {
    let open = ws.workshop.open;
    let view = |id: &'static str, label: &'static str, chosen: bool, workshop: bool| {
        choice(Button::new(id).small().label(label), chosen, cx).on_click(cx.listener(
            move |ws, _, _, cx| {
                ws.workshop.open = workshop;
                if workshop && ws.workshop.listings.is_none() {
                    ws.load_store(cx);
                }
                cx.notify();
            },
        ))
    };
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap_1()
        .max_w(px(880.))
        .child(view("plugins-installed", "Installed", !open, false))
        .child(view("plugins-workshop", "Workshop", open, true))
        .when(open, |row| {
            row.child(div().flex_1()).child(
                Button::new("workshop-refresh")
                    .ghost()
                    .small()
                    .icon(Icon::default().path(crate::ui::icons::REFRESH))
                    .loading(ws.workshop.loading)
                    .tooltip("Read the store again")
                    .on_click(cx.listener(|ws, _, _, cx| ws.load_store(cx))),
            )
        })
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
    let mut cards = grid();
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
