//! The device's name and runtime at the top of the panel; a click opens a menu of the
//! other devices.

use gpui_kit::{
    component::{ActiveTheme, Icon},
    prelude::FluentBuilder,
    *,
};

use super::Panel;
use crate::{
    ui::{Type, widgets},
    workspace::Workspace,
};

const CHEVRON: &str = "icons/chevron-down.svg";

pub fn button(panel: &Panel, cx: &mut Context<Workspace>) -> Stateful<Div> {
    let theme = cx.theme();
    let (muted, hover) = (theme.muted_foreground, theme.accent);
    let (name, runtime) = match panel.chosen_device() {
        Some(device) => (device.name.clone(), device.runtime.clone()),
        None => ("Choose a device".to_owned(), String::new()),
    };
    div()
        .id("simulator-picker")
        .flex()
        .flex_row()
        .items_center()
        .gap_1()
        .px_2()
        .py_1()
        .rounded_md()
        .cursor_pointer()
        .hover(move |row| row.bg(hover))
        .child(
            div()
                .flex()
                .flex_col()
                .child(div().small().font_weight(FontWeight::SEMIBOLD).child(name))
                .when(!runtime.is_empty(), |column| {
                    column.child(div().meta().text_color(muted).child(runtime))
                }),
        )
        .child(Icon::default().path(CHEVRON).size_4().text_color(muted))
        .on_click(cx.listener(|ws, _, _, cx| ws.toggle_device_menu(cx)))
}

/// The devices to choose from, over the panel under its top.
pub fn menu(panel: &Panel, cx: &mut Context<Workspace>) -> Deferred {
    let theme = cx.theme();
    let (surface, border, muted) = (theme.popover, theme.border, theme.muted_foreground);
    let devices = panel
        .found
        .as_ref()
        .map(|found| found.devices.as_slice())
        .unwrap_or_default();
    let mut list = div()
        .id("simulator-devices")
        .absolute()
        .top(px(46.))
        .left(px(10.))
        .w(px(260.))
        .max_h(px(420.))
        .overflow_y_scroll()
        .p_1()
        .rounded_lg()
        .border_1()
        .border_color(border)
        .bg(surface)
        .shadow_lg()
        .flex()
        .flex_col()
        .on_mouse_down_out(cx.listener(|ws, _, _, cx| {
            ws.simulator.picking = false;
            cx.notify();
        }));
    for (index, device) in devices.iter().enumerate() {
        let chosen = panel.chosen.as_ref() == Some(&device.udid);
        let udid = device.udid.clone();
        list = list.child(
            widgets::menu_item(("simulator-device", index), cx)
                .when(chosen, |row| row.font_weight(FontWeight::SEMIBOLD))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .small()
                        .child(device.name.clone()),
                )
                .child(div().meta().text_color(muted).child(if device.booted {
                    "on".to_owned()
                } else {
                    device.runtime.clone()
                }))
                .on_click(cx.listener(move |ws, _, _, cx| ws.choose_device(udid.clone(), cx))),
        );
    }
    widgets::on_top(list)
}
