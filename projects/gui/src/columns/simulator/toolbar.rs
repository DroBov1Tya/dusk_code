//! The buttons under the phone: the device's home and lock, Simulator.app, and power.
//! Without AXe the device takes no touches, and the way to install it is said here.

use dusk_protocol::system::SimButton;
use gpui_kit::{
    component::{
        ActiveTheme, Disableable, Icon,
        button::{Button, ButtonVariants},
    },
    *,
};

use super::{Panel, setup::hint};
use crate::{
    ui::{
        controls,
        icons::{HOME, LOCK, POWER},
    },
    workspace::Workspace,
};

const OPEN: &str = "icons/external-link.svg";
const AXE: &str = "brew install cameroncooke/axe/axe";

pub fn render(panel: &Panel, cx: &mut Context<Workspace>) -> Div {
    let booted = panel.chosen_device().is_some_and(|device| device.booted);
    let input = panel.input();
    let theme = cx.theme();
    let pill = div()
        .flex()
        .flex_row()
        .items_center()
        .gap_1()
        .p_1()
        .rounded_full()
        .border_1()
        .border_color(theme.border)
        .child(
            tool("simulator-home", HOME, "Home")
                .disabled(!booted || !input)
                .on_click(cx.listener(|ws, _, _, cx| ws.sim_button(SimButton::Home, cx))),
        )
        .child(
            tool("simulator-lock", LOCK, "Lock")
                .disabled(!booted || !input)
                .on_click(cx.listener(|ws, _, _, cx| ws.sim_button(SimButton::Lock, cx))),
        )
        .child(
            tool("simulator-app", OPEN, "Open in Simulator.app")
                .on_click(cx.listener(|ws, _, _, cx| ws.open_simulator_app(cx))),
        )
        .child(
            tool("simulator-power", POWER, "Shut down")
                .disabled(!booted)
                .on_click(cx.listener(|ws, _, _, cx| ws.power(false, cx))),
        );
    let mut column = div().flex().flex_col().items_center().gap_3().child(pill);
    if booted && !input {
        column = column.child(
            div()
                .w_full()
                .flex()
                .flex_col()
                .gap_2()
                .child(hint(
                    "Touches and keys need AXe, a free command line tool. Install it, then reopen the panel:",
                    cx,
                ))
                .child(controls::copy_field("simulator-axe", AXE.into(), cx)),
        );
    }
    column
}

fn tool(id: &'static str, icon: &'static str, tip: &'static str) -> Button {
    Button::new(id)
        .ghost()
        .rounded_full()
        .icon(Icon::default().path(icon))
        .tooltip(tip)
}
