//! The connectors page, in the middle column: MCP servers, skills, plugins, tools and
//! the agents' graph, each a tab, with one search over the open tab. The core saves
//! every switch; this page only asks.

mod card;
mod graph;
mod mcp;
mod plugins;
mod skills;
mod tools;
pub mod voice;

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
    Voice,
    Agents,
}

pub fn render(
    ws: &mut Workspace,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) -> Stateful<Div> {
    ws.voice_form.settle(window, cx);
    let Some(settings) = ws.settings.clone() else {
        return div()
            .id("connectors")
            .child(widgets::muted("Loading...", cx));
    };
    let counts = [
        (Shelf::Connectors, "Connectors", settings.mcp.len()),
        (Shelf::Skills, "Skills", settings.skills.len()),
        (Shelf::Plugins, "Plugins", ws.status.plugins.len()),
        (Shelf::Tools, "Tools", settings.tools.len()),
        (Shelf::Voice, "Voice", 0),
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
                    if shelf == Shelf::Voice {
                        if ws.voice.is_none() {
                            ws.load_voice(cx);
                        }
                        ws.load_voice_models(cx);
                    }
                    cx.notify();
                })),
        );
    }
    let query = ws.shelf_search.read(cx).value().trim().to_lowercase();
    let body = match ws.shelf {
        Shelf::Connectors => mcp::render(&settings, &ws.mcp_restarting, &query, cx),
        Shelf::Skills => skills::render(&settings, &query, cx),
        Shelf::Plugins => plugins::render(ws, &query, cx),
        Shelf::Tools => tools::render(&settings, &query, cx),
        Shelf::Voice => voice::render(ws, cx),
        Shelf::Agents => graph::render(ws, &settings.agents, cx),
    };
    div()
        .id("connectors")
        .size_full()
        .overflow_y_scroll()
        .child(
            div()
                .px_8()
                .py_6()
                .flex()
                .flex_col()
                .child(crate::ui::page_title(
                    "Connectors",
                    "What the agents can reach: servers, skills, plugins, tools, speech and other agents.",
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
                        .when(!matches!(ws.shelf, Shelf::Agents | Shelf::Voice), |row| {
                            row.child(div().w(px(280.)).child(Input::new(&ws.shelf_search)))
                        }),
                )
                .child(body),
        )
}

/// Cards wrap at the available width.
fn grid() -> Div {
    div().flex().flex_wrap().gap_3()
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
