//! The three steps before a device can run, each done or with the way to do it.

use dusk_protocol::system::Simulator;
use gpui_kit::{
    component::{
        ActiveTheme, IconName,
        button::{Button, ButtonVariants},
    },
    prelude::FluentBuilder,
    *,
};

use crate::{
    ui::{Type, widgets},
    workspace::Workspace,
};

const APP_STORE_XCODE: &str = "macappstore://apps.apple.com/app/id497799835";

pub fn render(found: &Simulator, cx: &mut Context<Workspace>) -> Div {
    let xcode = step(1, "Xcode", found.xcode, cx).when(!found.xcode, |step| {
        step.child(hint(
            "Install Xcode from the App Store, then open it once.",
            cx,
        ))
        .child(
            Button::new("simulator-xcode")
                .label("Open the App Store")
                .on_click(|_, _, cx| cx.open_url(APP_STORE_XCODE)),
        )
    });
    let has_runtime = !found.runtimes.is_empty();
    let runtime = step(2, "iOS runtime", has_runtime, cx)
        .when(has_runtime, |step| {
            step.child(hint(&found.runtimes.join(", "), cx))
        })
        .when(found.xcode && !has_runtime, |step| {
            let step = step.child(hint(
                "The simulator needs the iOS platform, a download of several gigabytes.",
                cx,
            ));
            match (found.installing, &found.install_failed) {
                (true, _) => step.child(crate::ui::loading("Downloading iOS", cx)),
                (false, failed) => step
                    .when_some(failed.clone(), |step, failed| {
                        step.child(hint(&format!("The last download failed: {failed}"), cx))
                    })
                    .child(
                        Button::new("simulator-runtime")
                            .primary()
                            .label("Download iOS")
                            .on_click(cx.listener(|ws, _, _, cx| ws.install_runtime(cx))),
                    ),
            }
        });
    let device = step(3, "A device", !found.devices.is_empty(), cx).when(
        has_runtime && found.devices.is_empty(),
        |step| {
            step.child(hint(
                "Add an iPhone in Xcode, under Window, Devices and Simulators.",
                cx,
            ))
        },
    );
    div()
        .flex()
        .flex_col()
        .gap_4()
        .child(xcode)
        .child(runtime)
        .child(device)
}

fn step(number: usize, title: &'static str, done: bool, cx: &App) -> Div {
    let theme = cx.theme();
    let mark = if done {
        widgets::icon(IconName::CircleCheck, theme.success).into_any_element()
    } else {
        div()
            .size(px(16.))
            .flex()
            .items_center()
            .justify_center()
            .rounded_full()
            .border_1()
            .border_color(theme.muted_foreground)
            .meta()
            .text_color(theme.muted_foreground)
            .child(number.to_string())
            .into_any_element()
    };
    // Buttons keep their own width rather than the column's.
    div().flex().flex_col().items_start().gap_2().child(
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .child(mark)
            .child(div().body().font_weight(FontWeight::MEDIUM).child(title)),
    )
}

pub fn hint(text: &str, cx: &App) -> Div {
    div()
        .small()
        .text_color(cx.theme().muted_foreground)
        .child(text.to_owned())
}
