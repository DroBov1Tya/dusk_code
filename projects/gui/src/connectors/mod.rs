//! The connectors page, in the middle column: MCP servers, skills, plugins, tools and
//! the agents' graph, each a tab, with one search over the open tab. The core saves
//! every switch; this page only asks.

mod card;
mod graph;
mod mcp;
mod plugins;
mod skills;
mod tools;

use dusk_protocol::{
    client::Switch as CoreSwitch,
    command::{Answer, Command},
};
use gpui_kit::{
    component::{ActiveTheme, Sizable, input::Input, switch::Switch},
    prelude::FluentBuilder,
    *,
};

use crate::{widgets, workspace::Workspace};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Shelf {
    Connectors,
    Skills,
    Plugins,
    Tools,
    Agents,
}

pub fn render(ws: &mut Workspace, _: &mut Window, cx: &mut Context<Workspace>) -> Stateful<Div> {
    let Some(settings) = ws.settings.clone() else {
        return div().id("connectors").child(widgets::muted("Loading...", cx));
    };
    let counts = [
        (Shelf::Connectors, "Connectors", settings.mcp.len()),
        (Shelf::Skills, "Skills", settings.skills.len()),
        (Shelf::Plugins, "Plugins", ws.status.plugins.len()),
        (Shelf::Tools, "Tools", settings.tools.len()),
        (Shelf::Agents, "Agents", 0),
    ];
    let theme = cx.theme();
    let (foreground, muted, accent, border) =
        (theme.foreground, theme.muted_foreground, theme.primary, theme.border);
    let mut tabs = div().flex().flex_row().gap_6().px_6().pt_5();
    for (shelf, label, count) in counts {
        let active = ws.shelf == shelf;
        tabs = tabs.child(
            div()
                .id(label)
                .flex()
                .flex_row()
                .items_baseline()
                .gap_1()
                .pb_1()
                .cursor_pointer()
                .border_b_2()
                .border_color(if active { accent } else { gpui_kit::transparent_black() })
                .text_color(if active { foreground } else { muted })
                .text_base()
                .child(label)
                .when_some((count > 0).then_some(count), |tab, count| {
                    tab.child(div().text_xs().text_color(muted).child(count.to_string()))
                })
                .on_click(cx.listener(move |ws, _, _, cx| {
                    ws.shelf = shelf;
                    cx.notify();
                })),
        );
    }
    let query = ws.shelf_search.read(cx).value().trim().to_lowercase();
    let body = match ws.shelf {
        Shelf::Connectors => mcp::render(&settings, &query, cx),
        Shelf::Skills => skills::render(&settings, &query, cx),
        Shelf::Plugins => plugins::render(ws, &query, cx),
        Shelf::Tools => tools::render(&settings, &query, cx),
        Shelf::Agents => graph::render(&settings.agents, cx),
    };
    div()
        .id("connectors")
        .size_full()
        .overflow_y_scroll()
        .child(tabs)
        .child(div().h(px(1.)).mt_3().bg(border))
        .child(
            div()
                .px_6()
                .py_4()
                .when(ws.shelf != Shelf::Agents, |page| {
                    page.child(div().mb_4().child(Input::new(&ws.shelf_search)))
                })
                .child(body),
        )
}

/// Two columns of cards.
fn grid() -> Div {
    div().grid().grid_cols(2).gap_3()
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
