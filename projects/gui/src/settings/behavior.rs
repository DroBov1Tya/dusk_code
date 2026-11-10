//! What the core lets the user change, page by page: the model, the chat, safety,
//! memory and the advanced switches.

use dusk_protocol::{client::Switch as CoreSwitch, config::ApprovalPolicy};
use gpui_kit::{
    component::{Sizable, button::Button, input::Input, switch::Switch},
    *,
};

use super::{SettingsPage, line};
use crate::{widgets, workspace::Workspace};

pub fn render(ws: &mut Workspace, page: SettingsPage, cx: &mut Context<Workspace>) -> Div {
    let mut body = div().flex().flex_col();
    match page {
        SettingsPage::Model => {
            body = body
                .child(line(
                    "Model",
                    &ws.status
                        .model
                        .clone()
                        .unwrap_or_else(|| "none chosen".to_owned()),
                    cx,
                ))
                .child(
                    line(
                        "Use a model by name",
                        "For servers that do not list their models: provider/model",
                        cx,
                    )
                    .child(div().w(px(260.)).child(Input::new(&ws.model_input)))
                    .child(
                        Button::new("model-use")
                            .small()
                            .label("Use")
                            .on_click(cx.listener(|ws, _, _, cx| ws.use_typed_model(cx))),
                    ),
                )
                .child(super::providers::render(ws, cx));
        }
        SettingsPage::Chat => {
            body = body.child(
                line("Mode", "What the chat may do and how it is told", cx).child(
                    Button::new("next-mode")
                        .small()
                        .label(ws.status.mode.clone())
                        .tooltip("Switch to the next mode")
                        .on_click(cx.listener(|ws, _, _, cx| ws.next_mode(cx))),
                ),
            );
        }
        SettingsPage::Notifications => {
            if let Some(settings) = ws.settings.clone() {
                body = body
                    .child(switch_line(
                        "Notifications",
                        "Tell when a turn ends or a question waits, while the window is away",
                        settings.notifications,
                        CoreSwitch::Notifications,
                        cx,
                    ))
                    .child(switch_line(
                        "Sound",
                        "A short sound with each notification",
                        settings.sound_notifications,
                        CoreSwitch::SoundNotifications,
                        cx,
                    ));
            }
        }
        SettingsPage::VersionControl => {
            if let Some(settings) = ws.settings.clone() {
                let prefix = settings.version_control.branch_prefix.clone();
                let current = if prefix.is_empty() {
                    "Branches the agents make have no prefix".to_owned()
                } else {
                    format!("Now: {prefix}")
                };
                body = body.child(
                    line("Branch prefix", &current, cx)
                        .child(div().w(px(200.)).child(Input::new(&ws.branch_input)))
                        .child(
                            Button::new("branch-prefix-save")
                                .small()
                                .label("Save")
                                .on_click(cx.listener(|ws, _, _, cx| ws.save_branch_prefix(cx))),
                        ),
                );
            }
        }
        SettingsPage::Safety => {
            for (label, hint, policy) in [
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
            ] {
                let on = ws.status.approvals == policy;
                body = body.child(
                    line(label, hint, cx).child(
                        Switch::new(SharedString::from(format!("policy-{label}")))
                            .checked(on)
                            .small()
                            .on_click(cx.listener(move |ws, on: &bool, _, cx| {
                                ws.switch(CoreSwitch::Approvals(policy), *on, cx);
                            })),
                    ),
                );
            }
            body = body.child(
                widgets::muted("With all off, the agent asks before risky actions.", cx).px_0(),
            );
        }
        SettingsPage::Memory => {
            if let Some(settings) = ws.settings.clone() {
                body = body
                    .child(switch_line(
                        "Memory",
                        "What the agents remember between chats",
                        settings.memory.enabled,
                        CoreSwitch::Memory,
                        cx,
                    ))
                    .child(switch_line(
                        "Work history",
                        "A summary of past work in each project",
                        settings.work_history.enabled,
                        CoreSwitch::WorkHistory,
                        cx,
                    ));
            }
        }
        SettingsPage::Advanced => {
            if let Some(settings) = ws.settings.clone() {
                body = body
                    .child(switch_line(
                        "Log files",
                        "Written on this machine only",
                        settings.logging,
                        CoreSwitch::Logging,
                        cx,
                    ))
                    .child(
                        line(
                            "Agents at work at once",
                            "Subagents that may run at the same time",
                            cx,
                        )
                        .child(stepper(
                            "agents",
                            settings.max_agents_at_work as u64,
                            1,
                            32,
                            |ws, value, cx| ws.set_limit(Limit::Agents, value, cx),
                            cx,
                        )),
                    )
                    .child(
                        line(
                            "Goal rounds",
                            "How many rounds a goal may take before it stops",
                            cx,
                        )
                        .child(stepper(
                            "rounds",
                            u64::from(settings.max_goal_rounds),
                            1,
                            200,
                            |ws, value, cx| ws.set_limit(Limit::Rounds, value, cx),
                            cx,
                        )),
                    );
            }
        }
        SettingsPage::Appearance
        | SettingsPage::AboutMe
        | SettingsPage::Keyboard
        | SettingsPage::Updates
        | SettingsPage::Usage => {}
    }
    body
}

#[derive(Clone, Copy)]
enum Limit {
    Agents,
    Rounds,
}

/// A number with a minus and a plus, kept inside its bounds.
fn stepper(
    id: &'static str,
    value: u64,
    min: u64,
    max: u64,
    set: fn(&mut Workspace, u64, &mut Context<Workspace>),
    cx: &mut Context<Workspace>,
) -> Div {
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .child(
            Button::new(SharedString::from(format!("{id}-less")))
                .xsmall()
                .label("−")
                .on_click(
                    cx.listener(move |ws, _, _, cx| set(ws, value.saturating_sub(1).max(min), cx)),
                ),
        )
        .child(div().w(px(32.)).text_center().child(value.to_string()))
        .child(
            Button::new(SharedString::from(format!("{id}-more")))
                .xsmall()
                .label("+")
                .on_click(cx.listener(move |ws, _, _, cx| set(ws, (value + 1).min(max), cx))),
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
            .small()
            .on_click(cx.listener(move |ws, on: &bool, _, cx| ws.switch(switch.clone(), *on, cx))),
    )
}

impl Workspace {
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
