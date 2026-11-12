//! The sidebar: the projects with their chats, and every chat by recent activity under
//! them. The ways into the rest of the app are on the rail beside it.

mod projects;
mod recent;
mod threads;

use gpui_kit::{component::ActiveTheme, *};

use crate::workspace::Workspace;

pub const WIDTH: f32 = 272.;

pub fn render(ws: &mut Workspace, window: &mut Window, cx: &mut Context<Workspace>) -> Div {
    let theme = cx.theme();
    let (panel, border) = (theme.sidebar, theme.sidebar_border);
    div()
        .w(px(WIDTH))
        .h_full()
        .flex()
        .flex_col()
        .bg(panel)
        .border_r_1()
        .border_color(border)
        .child(
            div()
                .h(px(crate::columns::topbar::HEIGHT))
                .flex_none()
                .flex()
                .items_center()
                .px_4()
                .child(wordmark(cx)),
        )
        .child(
            div()
                .id("chats")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .pb_4()
                .child(projects::render(ws, window, cx))
                .child(recent::render(ws, cx)),
        )
}

/// The name at the top: "Dusk" from violet to a sunset orange, "code" plain.
fn wordmark(cx: &App) -> Div {
    // Violet, magenta, rose, orange: one hue per letter along the dusk sky.
    const DUSK: [(f32, f32, f32); 4] = [
        (268., 0.72, 0.66),
        (300., 0.62, 0.64),
        (338., 0.74, 0.64),
        (24., 0.92, 0.60),
    ];
    let mut word = div()
        .flex()
        .flex_row()
        .text_lg()
        .font_weight(FontWeight::SEMIBOLD);
    for (letter, (hue, saturation, lightness)) in "Dusk".chars().zip(DUSK) {
        word = word.child(
            div()
                .text_color(hsla(hue / 360., saturation, lightness, 1.))
                .child(letter.to_string()),
        );
    }
    word.child(div().text_color(cx.theme().foreground).child("code"))
}
