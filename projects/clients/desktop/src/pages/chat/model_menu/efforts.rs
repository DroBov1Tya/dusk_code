//! The Effort submenu: how hard the model thinks.

use gpui_kit::{
    component::{ActiveTheme, IconName},
    prelude::FluentBuilder,
    *,
};

use super::{ROW, back_row};
use crate::pages::chat::model_bar::picker_panel;
use crate::{ui::widgets, workspace::Workspace};

const EFFORTS: &[&str] = &[
    "default", "off", "minimal", "low", "medium", "high", "xhigh", "max",
];

pub fn efforts(
    effort: &str,
    max_height: f32,
    width: f32,
    side_by_side: bool,
    cx: &mut Context<Workspace>,
) -> Div {
    let accent = cx.theme().primary;
    // Beside the menu the row it opened from already names it; alone, it needs the way back.
    let list = picker_panel(cx).w(px(width)).when(!side_by_side, |list| {
        list.child(back_row("Effort", side_by_side, cx))
    });
    let mut options = div()
        .id("effort-options")
        .flex()
        .flex_col()
        .max_h(px(max_height - 54.))
        .overflow_y_scroll();
    for level in EFFORTS {
        options = options.child(
            widgets::row(
                SharedString::from(format!("effort-{level}")),
                *level == effort,
                cx,
            )
            .role(Role::Button)
            .h(px(ROW))
            .flex_none()
            .when(*level == effort, |row| {
                row.aria_description("Selected effort")
            })
            .child(div().flex_1().child(widgets::capitalize(level)))
            .when(*level == effort, |row| {
                row.child(widgets::icon(IconName::Check, accent))
            })
            .on_click(cx.listener(move |ws, _, _, cx| ws.choose_effort(level, cx))),
        );
    }
    list.child(options)
}
