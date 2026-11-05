//! The detail window of a connector: an MCP server with each of its tools and their
//! switches, a skill or an agent with its file, a plugin with what it brings, a tool
//! with what it tells the model.

use dusk_protocol::{catalog::NodeText, client::Switch as CoreSwitch, mcp::McpState};
use gpui_kit::{
    component::{ActiveTheme, Sizable, switch::Switch},
    *,
};

use super::Modal;
use crate::{chat::links, widgets, workspace::Workspace};

#[derive(Clone)]
pub enum Detail {
    Mcp(String),
    Skill { name: String, file: Option<NodeText> },
    Agent { name: String, file: Option<NodeText> },
    Plugin(String),
    Tool(String),
}

impl Detail {
    pub fn title(&self) -> &str {
        match self {
            Self::Mcp(name) | Self::Plugin(name) | Self::Tool(name) => name,
            Self::Skill { name, .. } | Self::Agent { name, .. } => name,
        }
    }
}

pub fn render(ws: &Workspace, detail: &Detail, cx: &mut Context<Workspace>) -> Div {
    let theme = cx.theme();
    let (muted, success, danger, warning) =
        (theme.muted_foreground, theme.success, theme.danger, theme.warning);
    let page = div().flex().flex_col().gap_3().p_5();
    let Some(settings) = &ws.settings else {
        return page.child(widgets::muted("Loading...", cx));
    };
    match detail {
        Detail::Mcp(name) => {
            let Some(server) = settings.mcp.iter().find(|server| &server.name == name) else {
                return page.child(widgets::muted("This server is gone.", cx));
            };
            let (state, color) = match &server.state {
                McpState::Off => ("Off".to_owned(), muted),
                McpState::Starting => ("Starting".to_owned(), warning),
                McpState::Ready(_) => ("Ready".to_owned(), success),
                McpState::Failed(error) => (format!("Failed: {error}"), danger),
            };
            let mut page = page
                .child(div().text_sm().text_color(color).child(state))
                .child(div().text_xs().text_color(muted).child(server.target.clone()));
            if server.tools.is_empty() {
                page = page.child(widgets::muted("Its tools show once the server is ready.", cx).px_0());
            }
            for tool in &server.tools {
                let short = tool
                    .name
                    .split_once("__")
                    .map_or(tool.name.as_str(), |(_, short)| short)
                    .to_owned();
                page = page.child(row(
                    &short,
                    &tool.description,
                    toggle(&tool.name, tool.enabled, CoreSwitch::Tool(tool.name.clone()), cx),
                    cx,
                ));
            }
            page
        }
        Detail::Tool(name) => {
            let Some(tool) = settings.tools.iter().find(|tool| &tool.name == name) else {
                return page.child(widgets::muted("This tool is gone.", cx));
            };
            page.child(row(
                kind_label(tool.kind.name()),
                "",
                toggle(&tool.name, tool.enabled, CoreSwitch::Tool(tool.name.clone()), cx),
                cx,
            ))
            .child(links::markdown("tool-description", tool.description.clone()))
        }
        Detail::Plugin(name) => {
            let Some(plugin) = ws.status.plugins.iter().find(|plugin| &plugin.name == name) else {
                return page.child(widgets::muted("This plugin is gone.", cx));
            };
            let mut page = page
                .child(div().text_xs().text_color(muted).child(format!(
                    "Version {} · brings {}",
                    plugin.version,
                    plugin.parts.join(", ")
                )))
                .child(links::markdown("plugin-description", plugin.description.clone()));
            if !plugin.missing_settings.is_empty() {
                page = page.child(div().text_sm().text_color(warning).child(format!(
                    "Needs settings: {}",
                    plugin.missing_settings.join(", ")
                )));
            }
            page
        }
        Detail::Skill { file, .. } | Detail::Agent { file, .. } => match file {
            None => page.child(widgets::muted("Loading...", cx)),
            Some(file) => {
                let mut page = page;
                for (key, value) in &file.metadata {
                    page = page.child(
                        div()
                            .flex()
                            .flex_row()
                            .gap_3()
                            .text_sm()
                            .child(div().w(px(110.)).text_color(muted).child(key.clone()))
                            .child(div().flex_1().child(value.clone())),
                    );
                }
                page.child(links::markdown("node-text", body(&file.text)))
            }
        },
    }
}

/// The file without its front matter, which the rows above already show.
fn body(text: &str) -> String {
    text.strip_prefix("---")
        .and_then(|rest| rest.split_once("\n---"))
        .map_or(text, |(_, body)| body)
        .trim_start()
        .to_owned()
}

fn kind_label(kind: &str) -> &str {
    kind
}

fn row(name: &str, description: &str, control: impl IntoElement, cx: &App) -> Div {
    let theme = cx.theme();
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap_3()
        .py_2()
        .border_b_1()
        .border_color(theme.border)
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w_0()
                .child(div().font_family("Menlo").text_sm().child(name.to_owned()))
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .line_clamp(3)
                        .child(description.to_owned()),
                ),
        )
        .child(control)
}

fn toggle(id: &str, on: bool, switch: CoreSwitch, cx: &mut Context<Workspace>) -> Switch {
    Switch::new(SharedString::from(format!("detail-{id}")))
        .checked(on)
        .small()
        .on_click(cx.listener(move |ws, on: &bool, _, cx| ws.switch(switch.clone(), *on, cx)))
}

impl Workspace {
    pub fn open_detail(&mut self, detail: Detail, cx: &mut Context<Self>) {
        let name = detail.title().to_owned();
        let load = match &detail {
            Detail::Skill { .. } => Some(true),
            Detail::Agent { .. } => Some(false),
            _ => None,
        };
        self.modal = Some(Modal::Detail(detail));
        cx.notify();
        let Some(skill) = load else {
            return;
        };
        self.core.call(
            cx,
            move |client| async move {
                if skill {
                    client.catalog().skill(&name).await
                } else {
                    client.catalog().agent(&name).await
                }
            },
            |ws, result, cx| match result {
                Ok(text) => {
                    if let Some(Modal::Detail(Detail::Skill { file, .. } | Detail::Agent { file, .. })) =
                        &mut ws.modal
                    {
                        *file = Some(text);
                        cx.notify();
                    }
                }
                Err(error) => ws.fail(error, cx),
            },
        );
    }
}
