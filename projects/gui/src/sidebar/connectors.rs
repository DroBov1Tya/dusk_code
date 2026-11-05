//! What the agents connect to and work with: MCP servers, skills, plugins and agents,
//! each with its switch. The core saves every switch; this only asks.

use dusk_protocol::{
    catalog::AgentNode,
    client::Switch as CoreSwitch,
    command::{Answer, Command},
    mcp::McpState,
};
use gpui_kit::{
    component::{
        ActiveTheme, IconName, Sizable,
        button::{Button, ButtonVariants},
        switch::Switch,
    },
    *,
};

use crate::{widgets, workspace::Workspace};

pub fn render(ws: &Workspace, cx: &mut Context<Workspace>) -> Div {
    let Some(settings) = &ws.settings else {
        return div().child(widgets::muted("Loading...", cx));
    };
    let theme = cx.theme();
    let (success, warning, danger, muted) =
        (theme.success, theme.warning, theme.danger, theme.muted_foreground);
    let mut list = div().flex().flex_col().pb_4().child(widgets::heading("MCP servers", cx));
    if settings.mcp.is_empty() {
        list = list.child(widgets::muted("No servers in the config.", cx));
    }
    for server in &settings.mcp {
        let (color, state) = match &server.state {
            McpState::Off => (muted, "off".to_owned()),
            McpState::Starting => (warning, "starting".to_owned()),
            McpState::Ready(tools) => (success, format!("{} tools", tools.len())),
            McpState::Failed(_) => (danger, "failed".to_owned()),
        };
        let on = !matches!(server.state, McpState::Off);
        let name = server.name.clone();
        let restart = server.name.clone();
        list = list.child(
            line(&server.name, cx)
                .child(widgets::badge(state, color))
                .child(
                    Button::new(SharedString::from(format!("restart-{name}")))
                        .ghost()
                        .xsmall()
                        .icon(IconName::RefreshCw)
                        .tooltip("Restart")
                        .on_click(cx.listener(move |ws, _, _, cx| ws.restart_mcp(&restart, cx))),
                )
                .child(toggle(format!("mcp-{name}"), on, CoreSwitch::Mcp(name), cx)),
        );
    }
    list = list.child(widgets::heading("Skills", cx));
    for skill in &settings.skills {
        list = list.child(
            line(&skill.name, cx).child(toggle(
                format!("skill-{}", skill.name),
                skill.enabled,
                CoreSwitch::Skill(skill.name.clone()),
                cx,
            )),
        );
    }
    list = list.child(widgets::heading("Plugins", cx));
    if ws.status.plugins.is_empty() {
        list = list.child(widgets::muted("No plugins installed.", cx));
    }
    for plugin in &ws.status.plugins {
        let name = plugin.name.clone();
        let missing = (!plugin.missing_settings.is_empty())
            .then(|| widgets::badge("needs settings", warning));
        list = list.child(
            line(&format!("{} {}", plugin.name, plugin.version), cx)
                .children(missing)
                .child(
                    Switch::new(SharedString::from(format!("plugin-{name}")))
                        .checked(plugin.enabled)
                        .xsmall()
                        .on_click(cx.listener(move |ws, on: &bool, _, cx| {
                            ws.plugin_enabled(&name, *on, cx);
                        })),
                ),
        );
    }
    list = list.child(widgets::heading("Agents", cx));
    let mut agents = Vec::new();
    flatten(&settings.agents.root, 0, &mut agents);
    for (depth, agent) in agents {
        list = list.child(
            line(&agent.name, cx)
                .pl(px(8. + depth as f32 * 12.))
                .child(toggle(
                    format!("agent-{}", agent.name),
                    agent.enabled,
                    CoreSwitch::Agent(agent.name.clone()),
                    cx,
                )),
        );
    }
    list
}

fn line(name: &str, cx: &App) -> Div {
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .px_3()
        .py_1()
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_color(cx.theme().foreground)
                .child(name.to_owned()),
        )
}

fn toggle(id: String, on: bool, switch: CoreSwitch, cx: &mut Context<Workspace>) -> Switch {
    Switch::new(SharedString::from(id))
        .checked(on)
        .xsmall()
        .on_click(cx.listener(move |ws, on: &bool, _, cx| ws.switch(switch.clone(), *on, cx)))
}

/// The tree without its root, the chat itself, which is always on.
fn flatten<'a>(node: &'a AgentNode, depth: usize, out: &mut Vec<(usize, &'a AgentNode)>) {
    for child in &node.children {
        out.push((depth, child));
        flatten(child, depth + 1, out);
    }
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

    fn restart_mcp(&mut self, name: &str, cx: &mut Context<Self>) {
        let name = name.to_owned();
        self.core.call(
            cx,
            move |client| async move { client.catalog().restart_mcp(&name).await },
            |ws, result, cx| match result {
                Ok(_) => ws.load_settings(cx),
                Err(error) => ws.fail(error, cx),
            },
        );
    }
}
