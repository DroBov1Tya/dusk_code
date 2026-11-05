//! MCP servers as cards: how each stands, a restart, a sign-in and its switch.

use dusk_protocol::{client::Switch as CoreSwitch, mcp::McpState, settings::Settings};
use gpui_kit::{
    component::{
        ActiveTheme, IconName, Sizable,
        button::{Button, ButtonVariants},
    },
    prelude::FluentBuilder,
    *,
};

use super::{card, grid, matches, toggle};
use crate::{modal::detail::Detail, widgets, workspace::Workspace};

pub fn render(settings: &Settings, query: &str, cx: &mut Context<Workspace>) -> Div {
    let theme = cx.theme();
    let (success, warning, danger, muted) =
        (theme.success, theme.warning, theme.danger, theme.muted_foreground);
    let servers: Vec<_> = settings
        .mcp
        .iter()
        .filter(|server| matches(query, &[&server.name, &server.target]))
        .collect();
    if servers.is_empty() {
        return div().child(widgets::muted("No MCP servers. Add one to the config's mcp section.", cx));
    }
    let mut cards = grid();
    for server in servers {
        let (state, color) = match &server.state {
            McpState::Off => ("Off".to_owned(), muted),
            McpState::Starting => ("Starting".to_owned(), warning),
            McpState::Ready(tools) => (format!("Ready · {} tools", tools.len()), success),
            McpState::Failed(_) => ("Failed".to_owned(), danger),
        };
        let description = match &server.state {
            McpState::Failed(error) => error.clone(),
            _ => server.target.clone(),
        };
        let name = server.name.clone();
        let (restart, sign_in, open) = (name.clone(), name.clone(), name.clone());
        let actions = div()
            .flex()
            .flex_row()
            .items_center()
            .gap_1()
            .when(server.remote, |row| {
                row.child(
                    Button::new(SharedString::from(format!("sign-in-{name}")))
                        .ghost()
                        .xsmall()
                        .label("Sign in")
                        .on_click(cx.listener(move |ws, _, _, cx| ws.sign_in_mcp(&sign_in, cx))),
                )
            })
            .child(
                Button::new(SharedString::from(format!("restart-{name}")))
                    .ghost()
                    .xsmall()
                    .icon(IconName::RefreshCw)
                    .tooltip("Restart")
                    .on_click(cx.listener(move |ws, _, _, cx| ws.restart_mcp(&restart, cx))),
            )
            .child(toggle(
                format!("mcp-{name}"),
                !matches!(server.state, McpState::Off),
                CoreSwitch::Mcp(name.clone()),
                cx,
            ));
        let kind = if server.remote { "MCP · remote" } else { "MCP · local" };
        cards = cards.child(card::render(
            card::Card {
                id: SharedString::from(format!("mcp-card-{name}")),
                name: server.name.clone(),
                kind: kind.to_owned(),
                description,
                state: Some((state, color)),
            },
            actions,
            cx,
        )
        .on_click(cx.listener(move |ws, _, _, cx| ws.open_detail(Detail::Mcp(open.clone()), cx))));
    }
    cards
}

impl Workspace {
    fn restart_mcp(&mut self, name: &str, cx: &mut Context<Self>) {
        let name = name.to_owned();
        self.core.call(
            cx,
            move |client| async move { client.catalog().restart_mcp(&name).await },
            |ws, result, cx| match result {
                Ok(()) => ws.load_settings(cx),
                Err(error) => ws.fail(error, cx),
            },
        );
    }

    /// The core answers with the page that signs in; the system browser opens it.
    fn sign_in_mcp(&mut self, name: &str, cx: &mut Context<Self>) {
        let name = name.to_owned();
        self.core.call(
            cx,
            move |client| async move { client.catalog().sign_in_mcp(&name).await },
            |ws, result, cx| match result {
                Ok(url) if url.starts_with("https://") || url.starts_with("http://localhost") => {
                    cx.open_url(&url);
                }
                Ok(url) => ws.fail(anyhow::anyhow!("refused to open {url}"), cx),
                Err(error) => ws.fail(error, cx),
            },
        );
    }
}
