//! MCP servers as cards: how each stands, a restart, a sign-in and its switch.

use std::{collections::HashSet, time::Duration};

use dusk_protocol::{
    client::Switch as CoreSwitch,
    config::{McpState, Settings},
};
use gpui_kit::{
    component::{
        ActiveTheme, Disableable, IconName, Sizable,
        button::{Button, ButtonVariants},
    },
    prelude::FluentBuilder,
    *,
};

use super::{card, grid, matches, toggle};
use crate::{modal::detail::Detail, ui::widgets, workspace::Workspace};

const SHOWN: Duration = Duration::from_millis(800);
const POLL: Duration = Duration::from_secs(1);
/// A server that has not settled in this many polls is left as it is.
const POLLS: usize = 40;

pub fn render(
    settings: &Settings,
    restarting: &HashSet<String>,
    query: &str,
    cx: &mut Context<Workspace>,
) -> Div {
    let theme = cx.theme();
    let (success, warning, danger, muted) = (
        theme.success,
        theme.warning,
        theme.danger,
        theme.muted_foreground,
    );
    let servers: Vec<_> = settings
        .mcp
        .iter()
        .filter(|server| matches(query, &[&server.name, &server.target]))
        .collect();
    if servers.is_empty() {
        return div().child(crate::ui::empty(
            IconName::Network,
            if query.is_empty() {
                "No MCP servers"
            } else {
                "No matching connectors"
            },
            if query.is_empty() {
                "Add one to the mcp section of the config; it appears here with its tools."
            } else {
                "Search by a server name or address."
            },
            cx,
        ));
    }
    let mut cards = grid();
    for server in servers {
        let busy = restarting.contains(&server.name);
        let (state, color) = match &server.state {
            _ if busy => ("Restarting".to_owned(), warning),
            McpState::Off => ("Off".to_owned(), muted),
            McpState::Starting => ("Starting".to_owned(), warning),
            McpState::Ready(tools) => (
                format!("Ready · {}", widgets::count(tools.len(), "tool")),
                success,
            ),
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
                    .icon(if busy {
                        IconName::LoaderCircle
                    } else {
                        IconName::RefreshCw
                    })
                    .disabled(busy)
                    .tooltip("Restart")
                    .on_click(cx.listener(move |ws, _, _, cx| ws.restart_mcp(&restart, cx))),
            )
            .child(toggle(
                format!("mcp-{name}"),
                !matches!(server.state, McpState::Off),
                CoreSwitch::Mcp(name.clone()),
                cx,
            ));
        let kind = if server.remote {
            "MCP · remote"
        } else {
            "MCP · local"
        };
        cards = cards.child(
            card::render(
                card::Card {
                    id: SharedString::from(format!("mcp-card-{name}")),
                    name: server.name.clone(),
                    kind: kind.to_owned(),
                    description,
                    state: Some((state, color)),
                    icon: server.icon.clone(),
                },
                actions,
                cx,
            )
            .on_click(
                cx.listener(move |ws, _, _, cx| ws.open_detail(Detail::Mcp(open.clone()), cx)),
            ),
        );
    }
    cards
}

impl Workspace {
    /// The card says "Restarting" for at least `SHOWN`, so a server that fails at once
    /// still shows the attempt; then the status is read until nothing is starting.
    fn restart_mcp(&mut self, name: &str, cx: &mut Context<Self>) {
        let name = name.to_owned();
        self.mcp_restarting.insert(name.clone());
        cx.notify();
        let started = name.clone();
        self.core.call(
            cx,
            move |client| async move { client.catalog().restart_mcp(&started).await },
            move |ws, result, cx| match result {
                Ok(()) => {
                    ws.load_settings(cx);
                    cx.spawn(async move |ws, cx| {
                        cx.background_executor().timer(SHOWN).await;
                        let _ = ws.update(cx, |ws, cx| {
                            ws.mcp_restarting.remove(&name);
                            cx.notify();
                        });
                        for _ in 0..POLLS {
                            cx.background_executor().timer(POLL).await;
                            let Ok(starting) = ws.update(cx, |ws, cx| {
                                ws.load_settings(cx);
                                ws.settings.as_ref().is_some_and(|settings| {
                                    settings
                                        .mcp
                                        .iter()
                                        .any(|server| server.state == McpState::Starting)
                                })
                            }) else {
                                return;
                            };
                            if !starting {
                                return;
                            }
                        }
                    })
                    .detach();
                }
                Err(error) => {
                    ws.mcp_restarting.remove(&name);
                    ws.fail(error, cx)
                }
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
