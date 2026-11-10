//! Small pieces every column draws with.

use crate::ui::Type;
use gpui_kit::{
    component::{ActiveTheme, Icon, IconName, Sizable},
    prelude::FluentBuilder,
    *,
};

/// A section's label: sentence case in the muted color, closer to its items than to
/// the section above.
pub fn caption(text: impl Into<SharedString>, cx: &App) -> Div {
    crate::ui::group(text, cx)
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
        .h(px(crate::ui::ROW))
        .rounded_lg()
        .cursor_pointer()
        .when(active, |row| row.bg(selected))
        .hover(move |row| row.bg(hover))
}

pub fn icon(name: IconName, color: Hsla) -> Icon {
    Icon::new(name).small().text_color(color)
}

/// "1 file", "2 files": a count with its noun in the plural unless it is one.
pub fn count(n: usize, noun: &str) -> String {
    format!("{n} {noun}{}", if n == 1 { "" } else { "s" })
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

/// A quiet line. One that ends in "..." is a wait, and gets the turning ring.
pub fn muted(text: impl Into<SharedString>, cx: &App) -> Div {
    let text = text.into();
    if text.ends_with("...") {
        return crate::ui::loading(&text, cx);
    }
    div()
        .px_3()
        .py_2()
        .small()
        .text_color(cx.theme().muted_foreground)
        .child(text)
}

/// A menu or a list that opens over the page: painted after everything else, so no text
/// or button beside it shows through, and it takes the mouse so nothing below reacts.
/// Give it an opaque background too, such as the theme's popover color.
pub fn on_top(popup: impl IntoElement + 'static) -> Deferred {
    deferred(popup).with_priority(1)
}

/// A popup that fades in as it opens, in 140 ms: the click that opened it is answered at
/// once, and the menu does not jump into place. It plays when the element first appears.
pub fn appear<E>(id: impl Into<ElementId>, popup: E) -> AnimationElement<E>
where
    E: IntoElement + Styled + 'static,
{
    popup.with_animation(
        id,
        Animation::new(crate::ui::OPEN).with_easing(ease_out_quint()),
        |popup, delta| popup.opacity(delta),
    )
}

/// `12.3k` for a token count.
pub fn short(tokens: u64) -> String {
    match tokens {
        0..1_000 => tokens.to_string(),
        1_000..1_000_000 => format!("{:.1}k", tokens as f64 / 1e3),
        _ => format!("{:.1}M", tokens as f64 / 1e6),
    }
}

pub fn capitalize(text: &str) -> String {
    let mut chars = text.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}
