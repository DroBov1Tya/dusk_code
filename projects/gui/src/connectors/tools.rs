//! The built-in tools of the chat's mode as cards, grouped by what they work on; a card
//! opens what the tool tells the model. MCP tools live with their servers.

use dusk_protocol::{
    client::Switch as CoreSwitch,
    settings::{Settings, ToolKind},
};
use gpui_kit::{component::ActiveTheme, *};

use super::{card, grid, matches, toggle};
use crate::{modal::detail::Detail, widgets, workspace::Workspace};

pub fn render(settings: &Settings, query: &str, cx: &mut Context<Workspace>) -> Div {
    let muted = cx.theme().muted_foreground;
    let mut page = div().flex().flex_col().gap_2();
    let mut kinds: Vec<_> = settings
        .tools
        .iter()
        .map(|tool| tool.kind)
        .filter(|kind| *kind != ToolKind::Mcp)
        .collect();
    kinds.dedup();
    for kind in kinds {
        let tools: Vec<_> = settings
            .tools
            .iter()
            .filter(|tool| tool.kind == kind && matches(query, &[&tool.name, &tool.description]))
            .collect();
        if tools.is_empty() {
            continue;
        }
        let mut cards = grid();
        for tool in tools {
            let open = tool.name.clone();
            let line = tool
                .description
                .lines()
                .next()
                .unwrap_or_default()
                .to_owned();
            cards = cards.child(
                card::render(
                    card::Card {
                        id: SharedString::from(format!("tool-card-{}", tool.name)),
                        name: tool.name.clone(),
                        kind: format!("Built in · {}", kind.name()),
                        description: line,
                        state: None,
                        icon: None,
                    },
                    toggle(
                        format!("tool-{}", tool.name),
                        tool.enabled,
                        CoreSwitch::Tool(tool.name.clone()),
                        cx,
                    ),
                    cx,
                )
                .on_click(
                    cx.listener(move |ws, _, _, cx| ws.open_detail(Detail::Tool(open.clone()), cx)),
                ),
            );
        }
        page = page
            .child(widgets::caption(widgets::capitalize(kind.name()), cx).px_0())
            .child(cards);
    }
    page.child(div().pt_2().text_xs().text_color(muted).child(format!(
        "Switches apply to the {} mode. MCP tools are under Connectors.",
        settings.mode
    )))
}
