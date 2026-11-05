//! The tools of the chat's mode, grouped by what they work on, each with its switch.

use dusk_protocol::{client::Switch as CoreSwitch, settings::Settings};
use gpui_kit::{component::ActiveTheme, *};

use super::{grid, matches, toggle};
use crate::{widgets, workspace::Workspace};

pub fn render(settings: &Settings, query: &str, cx: &mut Context<Workspace>) -> Div {
    let border = cx.theme().border;
    let mut page = div().flex().flex_col().gap_2();
    let mut kinds: Vec<_> = settings.tools.iter().map(|tool| tool.kind).collect();
    kinds.dedup();
    for kind in kinds {
        let tools: Vec<_> = settings
            .tools
            .iter()
            .filter(|tool| tool.kind == kind && matches(query, &[&tool.name]))
            .collect();
        if tools.is_empty() {
            continue;
        }
        let mut cards = grid();
        for tool in tools {
            cards = cards.child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_3()
                    .px_3()
                    .py_2()
                    .rounded_lg()
                    .border_1()
                    .border_color(border)
                    .child(div().flex_1().min_w_0().truncate().font_family("Menlo").child(tool.name.clone()))
                    .child(toggle(
                        format!("tool-{}", tool.name),
                        tool.enabled,
                        CoreSwitch::Tool(tool.name.clone()),
                        cx,
                    )),
            );
        }
        page = page.child(widgets::heading(kind.name(), cx)).child(cards);
    }
    page.child(widgets::muted(format!("Switches apply to the {} mode.", settings.mode), cx))
}
