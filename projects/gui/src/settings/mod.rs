//! The settings tab: how the window looks, and what the core lets the user change.

use dusk_protocol::{client::Switch as CoreSwitch, config::ApprovalPolicy};
use gpui_kit::{
    component::{
        ActiveTheme, Sizable,
        button::Button,
        switch::Switch,
    },
    *,
};

use crate::{theme, widgets, workspace::Workspace};

pub fn render(ws: &mut Workspace, _: &mut Window, cx: &mut Context<Workspace>) -> Stateful<Div> {
    let current = theme::current(cx).name;
    let mut themes = div().flex().flex_row().flex_wrap().gap_2().px_3();
    for palette in theme::PALETTES {
        let name = palette.name;
        let swatch = |color: &str| {
            div()
                .size_4()
                .rounded_full()
                .bg(hex(color))
        };
        themes = themes.child(
            div()
                .id(SharedString::from(format!("theme-{name}")))
                .flex()
                .flex_col()
                .gap_2()
                .p_3()
                .w(px(150.))
                .rounded_lg()
                .border_1()
                .border_color(if name == current {
                    cx.theme().primary
                } else {
                    cx.theme().border
                })
                .bg(hex(palette.background))
                .cursor_pointer()
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .gap_1()
                        .child(swatch(palette.panel))
                        .child(swatch(palette.surface))
                        .child(swatch(palette.accent))
                        .child(swatch(palette.success)),
                )
                .child(
                    div()
                        .text_color(
                            hex(palette.foreground),
                        )
                        .child(name),
                )
                .on_click(cx.listener(move |_, _, _, cx| theme::choose(name, cx))),
        );
    }
    let mut page = div()
        .flex()
        .flex_col()
        .gap_2()
        .py_4()
        .max_w(px(820.))
        .mx_auto()
        .w_full()
        .child(div().px_3().text_lg().font_weight(FontWeight::SEMIBOLD).child("Settings"))
        .child(widgets::heading("Theme", cx))
        .child(themes)
        .child(widgets::heading("Mode", cx))
        .child(
            line("Chat mode", cx).child(
                Button::new("next-mode")
                    .small()
                    .label(ws.status.mode.clone())
                    .tooltip("Switch to the next mode")
                    .on_click(cx.listener(|ws, _, _, cx| ws.next_mode(cx))),
            ),
        )
        .child(widgets::heading("Approvals", cx));
    for (label, policy) in [
        ("Run commands in the sandbox", ApprovalPolicy::Sandbox),
        ("Let the arbiter decide", ApprovalPolicy::Arbiter),
        ("Full access, no questions", ApprovalPolicy::FullAccess),
    ] {
        let on = ws.status.approvals == policy;
        page = page.child(line(label, cx).child(
            Switch::new(SharedString::from(format!("policy-{label}")))
                .checked(on)
                .xsmall()
                .on_click(cx.listener(move |ws, on: &bool, _, cx| {
                    ws.switch(CoreSwitch::Approvals(policy), *on, cx);
                })),
        ));
    }
    if let Some(settings) = &ws.settings {
        page = page
            .child(widgets::heading("Memory and logs", cx))
            .child(switch_line("Memory", settings.memory.enabled, CoreSwitch::Memory, cx))
            .child(switch_line(
                "Work history",
                settings.work_history.enabled,
                CoreSwitch::WorkHistory,
                cx,
            ))
            .child(switch_line("Log files", settings.logging, CoreSwitch::Logging, cx))
            .child(widgets::heading("Limits", cx))
            .child(line(&format!("Agents at work at once: {}", settings.max_agents_at_work), cx))
            .child(line(&format!("Goal rounds: {}", settings.max_goal_rounds), cx));
    }
    div().id("settings").size_full().overflow_y_scroll().child(page)
}

fn line(label: &str, cx: &App) -> Div {
    div()
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .px_3()
        .py_1()
        .text_color(cx.theme().foreground)
        .child(label.to_owned())
}

fn switch_line(label: &'static str, on: bool, switch: CoreSwitch, cx: &mut Context<Workspace>) -> Div {
    line(label, cx).child(
        Switch::new(label)
            .checked(on)
            .xsmall()
            .on_click(cx.listener(move |ws, on: &bool, _, cx| ws.switch(switch.clone(), *on, cx))),
    )
}

impl Workspace {
    fn next_mode(&mut self, cx: &mut Context<Self>) {
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
}

fn hex(color: &str) -> Hsla {
    use gpui_kit::component::Colorize;
    Hsla::parse_hex(color).unwrap_or_default()
}
