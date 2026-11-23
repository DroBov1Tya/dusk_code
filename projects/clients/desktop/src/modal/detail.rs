//! The detail window of a connector: an MCP server with each of its tools and their
//! switches, a skill or an agent with its file, a plugin with what it brings, a tool
//! with what it tells the model.

use dusk_protocol::{
    catalog::{NodeText, SkillKind},
    client::Switch as CoreSwitch,
    config::McpState,
    plugin::Kind,
};
use gpui_kit::{
    component::{
        ActiveTheme, Sizable,
        button::{Button, ButtonVariants},
        input::InputState,
        switch::Switch,
    },
    *,
};

use super::Modal;
use crate::{
    pages::{chat::links, customize::plugins::Confirm},
    ui::{Type, widgets},
    workspace::Workspace,
};

#[derive(Clone)]
pub enum Detail {
    Mcp(String),
    Skill {
        name: String,
        file: Option<NodeText>,
    },
    Agent {
        name: String,
        file: Option<NodeText>,
    },
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
    let remove = remove(ws, detail, cx);
    let theme = cx.theme();
    let (muted, success, danger, warning) = (
        theme.muted_foreground,
        theme.success,
        theme.danger,
        theme.warning,
    );
    let page = div().flex().flex_col().gap_3().p_5();
    let Some(settings) = &ws.settings else {
        return page.child(widgets::muted("Loading...", cx));
    };
    let shown = match detail {
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
                .child(div().small().text_color(color).child(state))
                .child(div().meta().text_color(muted).child(server.target.clone()));
            if server.tools.is_empty() {
                page = page
                    .child(widgets::muted("Its tools show once the server is ready.", cx).px_0());
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
                    toggle(
                        &tool.name,
                        tool.enabled,
                        CoreSwitch::Tool(tool.name.clone()),
                        cx,
                    ),
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
                toggle(
                    &tool.name,
                    tool.enabled,
                    CoreSwitch::Tool(tool.name.clone()),
                    cx,
                ),
                cx,
            ))
            .child(links::markdown(
                "tool-description",
                tool.description.clone(),
                links::Links::Web,
            ))
        }
        Detail::Plugin(name) => {
            let Some(plugin) = ws.status.plugins.iter().find(|plugin| &plugin.name == name) else {
                return page.child(widgets::muted("This plugin is gone.", cx));
            };
            let mut page = page
                .child(div().meta().text_color(muted).child(format!(
                    "Version {} · brings {}",
                    plugin.version,
                    plugin.parts.join(", ")
                )))
                .child(links::markdown(
                    "plugin-description",
                    plugin.description.clone(),
                    links::Links::Web,
                ));
            if let Some(version) = &plugin.duskcode_version {
                page = page.child(
                    div()
                        .meta()
                        .text_color(muted)
                        .child(format!("Written for Dusk Code {version}")),
                );
            }
            if !plugin.missing_settings.is_empty() {
                page = page.child(div().small().text_color(warning).child(format!(
                    "Needs settings: {}",
                    plugin.missing_settings.join(", ")
                )));
            }
            for (key, input) in &ws.secret_inputs {
                let (plugin, key_name) = (plugin.name.clone(), key.clone());
                page = page.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(div().small().child(key.clone()))
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .gap_2()
                                .child(
                                    div()
                                        .flex_1()
                                        .child(gpui_kit::component::input::Input::new(input)),
                                )
                                .child(
                                    gpui_kit::component::button::Button::new(SharedString::from(
                                        format!("secret-{key}"),
                                    ))
                                    .label("Use for this run")
                                    .on_click(cx.listener(
                                        move |ws, _, window, cx| {
                                            ws.give_secret(&plugin, &key_name, window, cx);
                                        },
                                    )),
                                ),
                        ),
                );
            }
            for host in &plugin.choose {
                let (name, chosen) = (plugin.name.clone(), host.clone());
                page = page.child(
                    gpui_kit::component::button::Button::new(SharedString::from(format!(
                        "use-for-{host}"
                    )))
                    .label(format!("Use for {host}"))
                    .on_click(cx.listener(move |ws, _, _, cx| {
                        ws.use_for_host(&name, &chosen, cx);
                    })),
                );
            }
            if !ws.secret_inputs.is_empty() {
                page = page.child(div().meta().text_color(muted).child(
                    "Kept in memory on this machine until duskcode stops; never written to the config. Enter it again after a restart.",
                ));
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
                            .small()
                            .child(
                                div()
                                    .w(px(120.))
                                    .flex_none()
                                    .text_color(muted)
                                    .child(key.clone()),
                            )
                            .child(div().flex_1().min_w_0().child(value.clone())),
                    );
                }
                page.child(div().pt_2().small().child(links::markdown(
                    "node-text",
                    body(&file.text),
                    links::Links::Web,
                )))
            }
        },
    };
    shown.children(remove)
}

/// Remove, which asks first. A plugin or skill put in its folder by hand gets one too:
/// only the core knows what the Workshop installed, and it says why it refuses.
fn remove(ws: &Workspace, detail: &Detail, cx: &mut Context<Workspace>) -> Option<Div> {
    let confirm = match detail {
        Detail::Mcp(name) => {
            let server = ws.settings.as_ref()?.mcp.iter().find(|s| &s.name == name)?;
            match server.added_from {
                Some(_) => Confirm::Remove(name.clone(), Kind::Connector),
                None => Confirm::RemoveServer(name.clone()),
            }
        }
        Detail::Plugin(name) => Confirm::Remove(name.clone(), Kind::Plugin),
        Detail::Skill { name, .. } => {
            // A built-in skill comes with duskcode; there is nothing to remove.
            let skills = &ws.settings.as_ref()?.skills;
            if skills
                .iter()
                .any(|s| &s.name == name && s.kind == SkillKind::Builtin)
            {
                return None;
            }
            Confirm::Remove(name.clone(), Kind::Skill)
        }
        Detail::Agent { .. } | Detail::Tool(_) => return None,
    };
    if ws.workshop.busy.contains_key(detail.title()) {
        return None;
    }
    let failed = ws.workshop.failed.get(detail.title()).map(|error| {
        div()
            .flex_1()
            .small()
            .text_color(cx.theme().danger)
            .child(error.clone())
    });
    Some(
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap_3()
            .justify_end()
            .pt_2()
            .children(failed)
            .child(
                Button::new("detail-remove")
                    .danger()
                    .outline()
                    .label("Remove")
                    .on_click(cx.listener(move |ws, _, window, cx| {
                        ws.confirm_store(confirm.clone(), window, cx)
                    })),
            ),
    )
}

/// The file without its front matter, which the rows above already show.
fn body(text: &str) -> String {
    dusk_protocol::catalog::split_front_matter(text)
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
                .child(div().font_family("Menlo").small().child(name.to_owned()))
                .child(
                    div()
                        .meta()
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
    /// A plugin's window, with a hidden field for each setting it still lacks.
    pub fn open_plugin(&mut self, name: &str, window: &mut Window, cx: &mut Context<Self>) {
        let missing = self
            .status
            .plugins
            .iter()
            .find(|plugin| plugin.name == name)
            .map(|plugin| plugin.missing_settings.clone())
            .unwrap_or_default();
        self.secret_inputs = missing
            .into_iter()
            .map(|key| {
                let input = cx.new(|cx| {
                    InputState::new(window, cx)
                        .masked(true)
                        .placeholder("Value")
                });
                (key, input)
            })
            .collect();
        self.open_detail(Detail::Plugin(name.to_owned()), cx);
    }

    fn give_secret(
        &mut self,
        plugin: &str,
        key: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((_, input)) = self.secret_inputs.iter().find(|(name, _)| name == key) else {
            return;
        };
        let value = input.read(cx).value().trim().to_owned();
        if value.is_empty() {
            return;
        }
        // The field forgets the value at once; only the core holds it now.
        input.update(cx, |input, cx| input.set_value("", window, cx));
        let (plugin, key) = (plugin.to_owned(), key.to_owned());
        self.core.call(
            cx,
            move |client| async move { client.settings().plugin_secret(&plugin, &key, &value).await },
            |ws, result, cx| match result {
                Ok(()) => cx.notify(),
                Err(error) => ws.fail(error, cx),
            },
        );
    }

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
                    if let Some(Modal::Detail(
                        Detail::Skill { file, .. } | Detail::Agent { file, .. },
                    )) = &mut ws.modal
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
