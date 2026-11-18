//! The connectors page, in the middle column: MCP servers, skills, plugins, tools and
//! the agents' graph, each a tab, with one search over the open tab. The core saves
//! every switch; this page only asks.

mod card;
mod graph;
mod mcp;
pub mod plugins;
mod skills;
mod tools;

use crate::ui::elevation::{Elevation, Level};
use dusk_protocol::client::{Answer, Command, Switch as CoreSwitch};
use gpui_kit::{
    component::{ActiveTheme, Sizable, input::Input, switch::Switch},
    prelude::FluentBuilder,
    *,
};

use crate::{
    ui::{Type, widgets},
    workspace::Workspace,
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Shelf {
    Connectors,
    Skills,
    Plugins,
    Tools,
    Agents,
}

pub fn render(
    ws: &mut Workspace,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) -> Stateful<Div> {
    let Some(settings) = ws.settings.clone() else {
        return div()
            .id("connectors")
            .child(widgets::muted("Loading...", cx));
    };
    ws.ensure_store(cx);
    let counts = [
        (Shelf::Connectors, "Connectors", settings.mcp.len()),
        (Shelf::Skills, "Skills", settings.skills.len()),
        (Shelf::Plugins, "Plugins", ws.status.plugins.len()),
        (Shelf::Tools, "Tools", settings.tools.len()),
        (Shelf::Agents, "Agents", 0),
    ];
    let theme = cx.theme();
    let (foreground, muted, raised, surface) = (
        theme.foreground,
        theme.muted_foreground,
        theme.accent,
        theme.muted,
    );
    // The shelves as one segmented control: one of them is always chosen.
    let mut tabs = div()
        .flex()
        .flex_row()
        .flex_wrap()
        .gap_0p5()
        .p_0p5()
        .rounded_lg()
        .bg(surface);
    for (shelf, label, count) in counts {
        let active = ws.shelf == shelf;
        tabs = tabs.child(
            div()
                .id(label)
                .flex()
                .flex_row()
                .items_baseline()
                .gap_1p5()
                .px_3()
                .py_1()
                .rounded_md()
                .cursor_pointer()
                .small()
                .font_weight(FontWeight::MEDIUM)
                .when(active, |tab| tab.bg(raised))
                .text_color(if active { foreground } else { muted })
                .child(label)
                .when_some((count > 0).then_some(count), |tab, count| {
                    tab.child(div().meta().text_color(muted).child(count.to_string()))
                })
                .on_click(cx.listener(move |ws, _, _, cx| {
                    ws.shelf = shelf;
                    cx.notify();
                })),
        );
    }
    let query = ws.shelf_search.read(cx).value().trim().to_lowercase();
    let body = match ws.shelf {
        Shelf::Connectors => mcp::render(&settings, &ws.mcp_restarting, &query, cx),
        Shelf::Skills => skills::render(ws, &settings, &query, window, cx),
        Shelf::Plugins => plugins::render(ws, &query, window, cx),
        Shelf::Tools => tools::render(&settings, &query, cx),
        Shelf::Agents => graph::render(ws, &settings.agents, cx),
    };
    div()
        .id("connectors")
        .size_full()
        .overflow_y_scroll()
        .track_scroll(&ws.shelf_scroll)
        .child(
            div()
                .px_8()
                .py_8()
                .flex()
                .flex_col()
                .child(crate::ui::page_title(
                    "Connectors",
                    "What the agents can reach: servers, skills, plugins, tools and other agents.",
                    cx,
                ))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .flex_wrap()
                        .items_center()
                        .gap_3()
                        .pb_4()
                        .child(tabs)
                        .child(div().flex_1())
                        .when(ws.shelf != Shelf::Agents, |row| {
                            row.child(div().w(px(280.)).child(Input::new(&ws.shelf_search)))
                        })
                        // The Workshop's filters sit beside the search they narrow with.
                        .when(ws.discovering().is_some(), |row| {
                            row.child(plugins::workshop::filters::button(ws, cx))
                        }),
                )
                .child(body),
        )
}

/// One card of rows, as wide as the Workshop's grid at most.
fn grid(cx: &App) -> Div {
    div()
        .flex()
        .flex_col()
        .max_w(px(1184.))
        .p_1()
        .rounded_xl()
        .elevation(Level::Raised, cx)
}

fn matches(query: &str, texts: &[&str]) -> bool {
    query.is_empty() || texts.iter().any(|text| text.to_lowercase().contains(query))
}

fn toggle(id: String, on: bool, switch: CoreSwitch, cx: &mut Context<Workspace>) -> Switch {
    Switch::new(SharedString::from(id))
        .checked(on)
        .small()
        .on_click(cx.listener(move |ws, on: &bool, _, cx| ws.switch(switch.clone(), *on, cx)))
}

impl Workspace {
    pub fn load_settings(&mut self, cx: &mut Context<Self>) {
        self.core.call(
            cx,
            |client| async move { client.command(Command::Settings).await },
            |ws, result, cx| match result {
                Ok(Answer::Settings(settings)) => {
                    ws.settings = Some(*settings);
                    cx.notify();
                }
                Ok(_) => ws.fail(anyhow::anyhow!("the core gave no settings"), cx),
                Err(error) => ws.fail(error, cx),
            },
        );
        cx.notify();
    }

    pub fn switch(&mut self, switch: CoreSwitch, on: bool, cx: &mut Context<Self>) {
        self.core.call(
            cx,
            move |client| async move { client.settings().switch(switch, on).await },
            |ws, result, cx| match result {
                Ok(()) => ws.load_settings(cx),
                Err(error) => ws.fail(error, cx),
            },
        );
    }
}
