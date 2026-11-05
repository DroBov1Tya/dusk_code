//! Small pieces every column draws with.

use gpui_kit::{
    prelude::FluentBuilder,
    component::{ActiveTheme, Icon, IconName, Sizable},
    *,
};

/// A column's heading: small capitals in the muted color.
pub fn heading(text: impl Into<SharedString>, cx: &App) -> Div {
    div()
        .px_3()
        .pt_3()
        .pb_1()
        .text_xs()
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(cx.theme().muted_foreground)
        .child(text.into().to_uppercase())
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
        .py_1()
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

/// A tab button of a column's tab bar.
pub fn tab(id: impl Into<ElementId>, label: &'static str, active: bool, cx: &App) -> Stateful<Div> {
    let theme = cx.theme();
    let color = if active {
        theme.foreground
    } else {
        theme.muted_foreground
    };
    div()
        .id(id)
        .px_2()
        .py_1()
        .rounded_md()
        .text_xs()
        .cursor_pointer()
        .text_color(color)
        .when(active, |tab| tab.bg(theme.tab_active))
        .hover(|tab| tab.text_color(theme.foreground))
        .child(label)
}
