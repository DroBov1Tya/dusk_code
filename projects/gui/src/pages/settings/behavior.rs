//! What the core lets the user change, page by page: the model, the chat, safety,
//! memory and the advanced switches.

use dusk_protocol::{
    client::Switch as CoreSwitch,
    config::{ApprovalPolicy, ContextPolicy, ContextPreset},
};
use gpui_kit::{
    component::{
        ActiveTheme, Disableable, IconName,
        button::{Button, ButtonVariants},
        input::Input,
        switch::Switch,
    },
    *,
};

use super::{SettingsPage, line, list};
use crate::{
    ui::{Type, controls, widgets},
    workspace::Workspace,
};

pub fn render(ws: &mut Workspace, page: SettingsPage, cx: &mut Context<Workspace>) -> Div {
    let body = div().flex().flex_col().gap_6();
    match page {
        SettingsPage::Model => {
            let model = ws
                .status
                .model
                .clone()
                .unwrap_or_else(|| "none chosen".to_owned());
            body.child(list(
                [
                    line("Model", &model, cx),
                    line(
                        "Use a model by name",
                        "For servers that do not list their models: provider/model",
                        cx,
                    )
                    .child(div().w(px(240.)).child(Input::new(&ws.model_input)))
                    .child(
                        Button::new("model-use")
                            .outline()
                            .label("Use")
                            .on_click(cx.listener(|ws, _, _, cx| ws.use_typed_model(cx))),
                    ),
                ],
                cx,
            ))
            .child(super::providers::render(ws, cx))
        }
        SettingsPage::Chat => body.child(list(
            [
                line("Mode", "What the chat may do and how it is told", cx).child(
                    div()
                        .relative()
                        .child(
                            Button::new("settings-mode")
                                .outline()
                                .label(ws.status.mode.clone())
                                .dropdown_caret(true)
                                .tooltip("Pick a mode")
                                .on_click(cx.listener(|ws, _, _, cx| {
                                    ws.toggle_picker(crate::workspace::Picker::Mode, cx)
                                })),
                        )
                        .children(
                            matches!(ws.picker, Some(crate::workspace::Picker::Mode)).then(|| {
                                widgets::on_top(crate::pages::chat::mode_menu::render(ws, true, cx))
                            }),
                        ),
                ),
            ],
            cx,
        )),
        SettingsPage::Notifications => match ws.settings.clone() {
            Some(settings) => body.child(list(
                [
                    switch_line(
                        "Notifications",
                        "Tell when a turn ends or a question waits, while the window is away",
                        settings.notifications,
                        CoreSwitch::Notifications,
                        cx,
                    ),
                    switch_line(
                        "Sound",
                        "A short sound with each notification",
                        settings.sound_notifications,
                        CoreSwitch::SoundNotifications,
                        cx,
                    ),
                ],
                cx,
            )),
            None => body,
        },
        SettingsPage::VersionControl => match ws.settings.clone() {
            Some(settings) => {
                let prefix = settings.version_control.branch_prefix.clone();
                let current = if prefix.is_empty() {
                    "Branches the agents make have no prefix".to_owned()
                } else {
                    format!("Now: {prefix}")
                };
                body.child(list(
                    [line("Branch prefix", &current, cx)
                        .child(div().w(px(200.)).child(Input::new(&ws.branch_input)))
                        .child(
                            Button::new("branch-prefix-save")
                                .outline()
                                .label("Save")
                                .on_click(cx.listener(|ws, _, _, cx| ws.save_branch_prefix(cx))),
                        )],
                    cx,
                ))
            }
            None => body,
        },
        SettingsPage::Safety => {
            let rows = [
                (
                    "Sandbox",
                    "Commands run confined to the project",
                    ApprovalPolicy::Sandbox,
                ),
                (
                    "Arbiter",
                    "A second model answers the approvals",
                    ApprovalPolicy::Arbiter,
                ),
                (
                    "Full access",
                    "No questions; only for trusted work",
                    ApprovalPolicy::FullAccess,
                ),
            ]
            .map(|(label, hint, policy)| {
                let on = ws.status.approvals == policy;
                line(label, hint, cx).child(
                    Switch::new(SharedString::from(format!("policy-{label}")))
                        .checked(on)
                        .on_click(cx.listener(move |ws, on: &bool, _, cx| {
                            ws.switch(CoreSwitch::Approvals(policy), *on, cx);
                        })),
                )
            });
            body.child(list(rows, cx)).child(
                widgets::muted("With all off, the agent asks before risky actions.", cx).px_1(),
            )
        }
        SettingsPage::Memory => match ws.settings.clone() {
            Some(settings) => body.child(list([context_line(ws, cx)], cx)).child(list(
                [
                    switch_line(
                        "Memory",
                        "What the agents remember between chats",
                        settings.memory.enabled,
                        CoreSwitch::Memory,
                        cx,
                    ),
                    size_line(
                        "Memory size",
                        "Characters kept; every request carries them",
                        settings.memory.max_chars,
                        true,
                        cx,
                    ),
                    switch_line(
                        "Work history",
                        "A summary of past work in each project",
                        settings.work_history.enabled,
                        CoreSwitch::WorkHistory,
                        cx,
                    ),
                    size_line(
                        "Work history size",
                        "Characters kept for each project",
                        settings.work_history.max_chars,
                        false,
                        cx,
                    ),
                ],
                cx,
            )),
            None => body,
        },
        SettingsPage::Advanced => {
            let body = match ws.settings.clone() {
                Some(settings) => {
                    let agents = settings.max_agents_at_work as u64;
                    let rounds = u64::from(settings.max_goal_rounds);
                    let mut web = vec![web_line(ws, cx)];
                    web.extend(web_address(ws, cx));
                    body.child(list(
                        [switch_line(
                            "Log files",
                            "Written on this machine only",
                            settings.logging,
                            CoreSwitch::Logging,
                            cx,
                        )],
                        cx,
                    ))
                    .child(list(web, cx))
                    .child(list(
                        [
                            line(
                                "Agents at work at once",
                                "Subagents that may run at the same time",
                                cx,
                            )
                            .child(stepper(
                                "agents",
                                agents,
                                (1, 32),
                                Limit::Agents,
                                cx,
                            )),
                            line(
                                "Goal rounds",
                                "How many rounds a goal may take before it stops",
                                cx,
                            )
                            .child(stepper(
                                "rounds",
                                rounds,
                                (1, 200),
                                Limit::Rounds,
                                cx,
                            )),
                        ],
                        cx,
                    ))
                }
                None => body,
            };
            body.child(super::uninstall::section(cx))
        }
        SettingsPage::Appearance
        | SettingsPage::AboutMe
        | SettingsPage::Keyboard
        | SettingsPage::Updates
        | SettingsPage::Usage
        | SettingsPage::Secrets => body,
    }
}

/// How much of the model's window a request may fill: the model's own, or a fixed size.
fn context_line(ws: &Workspace, cx: &mut Context<Workspace>) -> Div {
    let current = ws.status.context_policy.unwrap_or_default();
    let choices = std::iter::once(("Auto", ContextPolicy::Auto)).chain(
        ContextPreset::ALL
            .into_iter()
            .map(|preset| (preset.name(), ContextPolicy::Fixed(preset))),
    );
    let mut row = div().flex().flex_row().gap_1();
    for (label, policy) in choices {
        let button = Button::new(SharedString::from(format!("context-{label}")))
            .compact()
            .label(label)
            .on_click(cx.listener(move |ws, _, _, cx| ws.set_context_policy(policy, cx)));
        row = row.child(if policy == current {
            button.primary()
        } else {
            button.ghost()
        });
    }
    line(
        "Context window",
        "The largest request: the model's own window, or a fixed size",
        cx,
    )
    .child(row)
}

/// A size in characters as a few steps; one the config holds beside them shows as it is.
fn size_line(
    label: &'static str,
    hint: &str,
    current: usize,
    memory: bool,
    cx: &mut Context<Workspace>,
) -> Div {
    let mut sizes = vec![1000, 2000, 4000, 8000];
    if !sizes.contains(&current) {
        sizes.push(current);
        sizes.sort_unstable();
    }
    let mut row = div().flex().flex_row().gap_1();
    for size in sizes {
        let button = Button::new(SharedString::from(format!("{label}-{size}")))
            .compact()
            // Whole thousands read as 2k, not 2.0k.
            .label(match size % 1000 {
                0 => format!("{}k", size / 1000),
                _ => widgets::short(size as u64),
            })
            .on_click(cx.listener(move |ws, _, _, cx| ws.set_chars(memory, size, cx)));
        row = row.child(if size == current {
            button.primary()
        } else {
            button.ghost()
        });
    }
    line(label, hint, cx).child(row)
}

#[derive(Clone, Copy)]
enum Limit {
    Agents,
    Rounds,
}

/// A limit's number with a minus and a plus, kept inside its bounds.
fn stepper(
    id: &'static str,
    value: u64,
    (min, max): (u64, u64),
    limit: Limit,
    cx: &mut Context<Workspace>,
) -> Div {
    let less =
        cx.listener(move |ws, _, _, cx| ws.set_limit(limit, value.saturating_sub(1).max(min), cx));
    let more = cx.listener(move |ws, _, _, cx| ws.set_limit(limit, (value + 1).min(max), cx));
    controls::stepper(id, value, (min, max), less, more, cx)
}

/// The switch that serves this core to browsers on this machine, with its port.
fn web_line(ws: &Workspace, cx: &mut Context<Workspace>) -> Div {
    line(
        "Web access",
        "Opens the chats in a browser on this machine, beside this window",
        cx,
    )
    .child(
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .child(
                div()
                    .meta()
                    .text_color(cx.theme().muted_foreground)
                    .child("Port"),
            )
            .child(div().w(px(84.)).child(Input::new(&ws.web_port))),
    )
    .child(
        Switch::new("web-access")
            .checked(ws.web.is_some())
            .disabled(ws.web_busy)
            .on_click(cx.listener(|ws, on: &bool, _, cx| ws.set_web(*on, cx))),
    )
}

/// While the server runs: the address to open once, with the token of this run in it.
/// It takes the row's whole width under its name, so a long token never pushes the
/// buttons out of the card.
fn web_address(ws: &Workspace, cx: &mut Context<Workspace>) -> Option<Div> {
    let url = ws.web.as_ref()?.sign_in_url();
    let opened = url.clone();
    let muted = cx.theme().muted_foreground;
    Some(
        div()
            .flex()
            .flex_col()
            .gap_2p5()
            .px_4()
            .py_3()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_0p5()
                    .child(div().body().child("Address"))
                    .child(div().small().text_color(muted).child(
                        "Open it once; the part after # signs in this browser for this run",
                    )),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .child(div().flex_1().min_w_0().child(controls::copy_field(
                        "web-copy",
                        url.into(),
                        cx,
                    )))
                    .child(
                        Button::new("web-open")
                            .outline()
                            .icon(IconName::ExternalLink)
                            .label("Open")
                            .tooltip("Open it in the default browser")
                            .on_click(cx.listener(move |_, _, _, cx| cx.open_url(&opened))),
                    ),
            ),
    )
}

fn switch_line(
    label: &'static str,
    hint: &str,
    on: bool,
    switch: CoreSwitch,
    cx: &mut Context<Workspace>,
) -> Div {
    line(label, hint, cx).child(
        Switch::new(label)
            .checked(on)
            .on_click(cx.listener(move |ws, on: &bool, _, cx| ws.switch(switch.clone(), *on, cx))),
    )
}

impl Workspace {
    /// Starts or stops the web server of this core. It listens on this machine only.
    fn set_web(&mut self, on: bool, cx: &mut Context<Self>) {
        if self.web_busy {
            return;
        }
        let typed = self.web_port.read(cx).value().trim().to_owned();
        let port = match typed.as_str() {
            "" => dusk_web::DEFAULT_PORT,
            text => match text.parse::<u16>() {
                Ok(port) if port > 0 => port,
                _ => {
                    self.fail(anyhow::anyhow!("{text:?} is not a port number"), cx);
                    return;
                }
            },
        };
        self.web_busy = true;
        if on {
            self.core.call_hub(
                cx,
                move |hub| async move {
                    let config = dusk_web::Config {
                        bind: ([127, 0, 0, 1], port).into(),
                        ..dusk_web::Config::default()
                    };
                    dusk_web::start(hub, config).await
                },
                |ws, result, cx| {
                    ws.web_busy = false;
                    match result {
                        Ok(server) => ws.web = Some(server),
                        Err(error) => ws.fail(error, cx),
                    }
                    cx.notify();
                },
            );
        } else if let Some(server) = self.web.take() {
            self.core.call_hub(
                cx,
                |_| async move {
                    server.stop().await;
                    anyhow::Ok(())
                },
                |ws, _, cx| {
                    ws.web_busy = false;
                    cx.notify();
                },
            );
        } else {
            self.web_busy = false;
        }
    }

    pub fn next_mode(&mut self, cx: &mut Context<Self>) {
        self.core.call(
            cx,
            |client| async move { client.settings().next_mode().await },
            |ws, result, cx| {
                if let Err(error) = result {
                    ws.fail(error, cx);
                }
            },
        );
    }

    fn set_context_policy(&mut self, policy: ContextPolicy, cx: &mut Context<Self>) {
        self.core.call(
            cx,
            move |client| async move { client.settings().context_policy(policy).await },
            move |ws, result, cx| match result {
                // The core answers with the limit the policy gives; the ring shows it.
                Ok(limit) => {
                    ws.status.context_policy = Some(policy);
                    ws.status.input_limit = limit;
                    cx.notify();
                }
                Err(error) => ws.fail(error, cx),
            },
        );
    }

    fn set_chars(&mut self, memory: bool, chars: usize, cx: &mut Context<Self>) {
        self.core.call(
            cx,
            move |client| async move {
                match memory {
                    true => client.settings().memory_chars(chars).await,
                    false => client
                        .settings()
                        .work_history_chars(chars)
                        .await
                        .map(|_| ()),
                }
            },
            |ws, result, cx| match result {
                Ok(()) => ws.load_settings(cx),
                Err(error) => ws.fail(error, cx),
            },
        );
    }

    fn set_limit(&mut self, limit: Limit, value: u64, cx: &mut Context<Self>) {
        self.core.call(
            cx,
            move |client| async move {
                match limit {
                    Limit::Agents => client.settings().max_agents_at_work(value as usize).await,
                    Limit::Rounds => client.settings().max_goal_rounds(value as u32).await,
                }
            },
            |ws, result, cx| match result {
                Ok(()) => ws.load_settings(cx),
                Err(error) => ws.fail(error, cx),
            },
        );
    }

    pub fn save_branch_prefix(&mut self, cx: &mut Context<Self>) {
        let prefix = self.branch_input.read(cx).value().trim().to_owned();
        self.core.call(
            cx,
            move |client| async move { client.settings().branch_prefix(&prefix).await },
            |ws, result, cx| match result {
                Ok(()) => ws.load_settings(cx),
                Err(error) => ws.fail(error, cx),
            },
        );
    }

    /// A model the provider does not list, typed as provider/model.
    pub fn use_typed_model(&mut self, cx: &mut Context<Self>) {
        let name = self.model_input.read(cx).value().trim().to_owned();
        if name
            .split_once('/')
            .is_none_or(|(p, m)| p.is_empty() || m.is_empty())
        {
            self.fail(anyhow::anyhow!("write the model as provider/model"), cx);
            return;
        }
        self.choose_model(name, cx);
    }
}
