//! Small pieces every column draws with.

use gpui_kit::{
    component::{ActiveTheme, Icon, IconName, Sizable},
    prelude::FluentBuilder,
    *,
};

/// A section's label: sentence case in the muted color, closer to its items than to
/// the section above.
pub fn caption(text: impl Into<SharedString>, cx: &App) -> Div {
    div()
        .px_4()
        .pt_6()
        .pb_2()
        .text_xs()
        .font_weight(FontWeight::MEDIUM)
        .text_color(cx.theme().muted_foreground)
        .child(text.into())
}

pub fn heading(text: impl Into<SharedString>, cx: &App) -> Div {
    caption(text, cx)
}

/// A row that highlights under the pointer and when `active`.
pub fn row(id: impl Into<ElementId>, active: bool, cx: &App) -> Stateful<Div> {
    let theme = cx.theme();
    let (hover, selected) = (theme.list_hover, theme.list_active);
    div()
        .id(id)
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .mx_1()
        .px_2()
        .h(px(32.))
        .rounded_md()
        .cursor_pointer()
        .when(active, |row| row.bg(selected))
        .hover(move |row| row.bg(hover))
}

pub fn icon(name: IconName, color: Hsla) -> Icon {
    Icon::new(name).small().text_color(color)
}

/// A short label in a tinted pill, such as a chat's source.
pub fn badge(text: impl Into<SharedString>, color: Hsla) -> Div {
    div()
        .px_1p5()
        .rounded_sm()
        .text_xs()
        .text_color(color)
        .bg(color.opacity(0.14))
        .child(text.into())
}

pub fn muted(text: impl Into<SharedString>, cx: &App) -> Div {
    div()
        .px_3()
        .py_2()
        .text_xs()
        .text_color(cx.theme().muted_foreground)
        .child(text.into())
}

/// A menu or a list that opens over the page: painted after everything else, so no text
/// or button beside it shows through, and it takes the mouse so nothing below reacts.
/// Give it an opaque background too, such as the theme's popover color.
pub fn on_top(popup: impl IntoElement + 'static) -> Deferred {
    deferred(popup).with_priority(1)
}
