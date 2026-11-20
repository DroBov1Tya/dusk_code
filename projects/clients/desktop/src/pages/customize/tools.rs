//! The built-in tools of the chat's mode as rows, a card of them for each thing they
//! work on; a row opens what the tool tells the model. MCP tools live with their servers.

use crate::ui::Type;
use dusk_protocol::{
    client::Switch as CoreSwitch,
    config::{Settings, ToolKind},
};
use gpui_kit::{
    component::{ActiveTheme, IconName},
    *,
};

use super::{card, grid, matches, toggle};
use crate::{modal::detail::Detail, ui::widgets, workspace::Workspace};

pub fn render(settings: &Settings, query: &str, cx: &mut Context<Workspace>) -> Div {
    let muted = cx.theme().muted_foreground;
    let mut page = div().flex().flex_col().gap_2();
    let mut kinds = Vec::new();
    for tool in &settings.tools {
        if tool.kind != ToolKind::Mcp && !kinds.contains(&tool.kind) {
            kinds.push(tool.kind);
        }
    }
    let mut has_results = false;
    for kind in kinds {
        let tools: Vec<_> = settings
            .tools
            .iter()
            .filter(|tool| tool.kind == kind && matches(query, &[&tool.name, &tool.description]))
            .collect();
        if tools.is_empty() {
            continue;
        }
        has_results = true;
        let mut cards = grid(cx);
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
                        kind: "Built in".to_owned(),
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
    if !has_results {
        return page.child(crate::ui::empty(
            IconName::SquareTerminal,
            "No tool has that name or does that.",
            Some(super::clear_search("tools-empty-clear", cx)),
            cx,
        ));
    }
    page.child(div().pt_2().meta().text_color(muted).child(format!(
        "Switches apply to the {} mode. MCP tools are under Connectors.",
        settings.mode
    )))
}
