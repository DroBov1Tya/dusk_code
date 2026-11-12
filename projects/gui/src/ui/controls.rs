//! Controls made of several parts, drawn the same wherever they appear: a number with
//! minus and plus, a value to copy, a measured figure. Each part is at least 28 px, the
//! size a pointer finds without aiming.

use gpui_kit::{
    component::{
        ActiveTheme, Disableable, Icon, IconName, Sizable,
        button::{Button, ButtonVariants},
    },
    *,
};

use crate::ui::Type;

/// The height of a control on a settings row.
pub const CONTROL: f32 = crate::ui::size::CONTROL;

type Click = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

/// A number between a minus and a plus, joined in one rounded field as on macOS. A side
/// at its bound is greyed out, so the field says where the range ends.
pub fn stepper(
    id: &'static str,
    value: u64,
    (min, max): (u64, u64),
    less: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    more: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    cx: &App,
) -> Div {
    let theme = cx.theme();
    let side = |key: &str, icon: IconName, enabled: bool, click: Click| {
        Button::new(SharedString::from(format!("{id}-{key}")))
            .ghost()
            .icon(Icon::new(icon).small())
            .with_size(px(CONTROL))
            .disabled(!enabled)
            .on_click(move |event, window, cx| click(event, window, cx))
    };
    div()
        .flex()
        .flex_row()
        .items_center()
        .h(px(CONTROL))
        .rounded_lg()
        .border_1()
        .border_color(theme.border)
        .bg(theme.background)
        .overflow_hidden()
        .child(side("less", IconName::Minus, value > min, Box::new(less)))
        .child(
            div()
                .min_w(px(44.))
                .h_full()
                .flex()
                .items_center()
                .justify_center()
                .border_x_1()
                .border_color(theme.border)
                .body()
                .font_weight(FontWeight::MEDIUM)
                .child(value.to_string()),
        )
        .child(side("more", IconName::Plus, value < max, Box::new(more)))
}

/// A value to read and copy, such as an address: one line in the code face on a sunken
/// field, cut with "..." when it is long, and a copy button inside the field's right end.
/// The whole value goes to the clipboard, not the part shown.
pub fn copy_field(id: &'static str, value: SharedString, cx: &App) -> Div {
    let theme = cx.theme();
    let copied = value.clone();
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .w_full()
        .min_w_0()
        .h(px(CONTROL + 4.))
        .pl_3()
        .pr_1()
        .rounded_lg()
        .border_1()
        .border_color(theme.border)
        .bg(theme.background)
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .font_family(theme.mono_font_family.clone())
                .small()
                .text_color(theme.foreground)
                .child(value),
        )
        .child(
            Button::new(id)
                .ghost()
                .small()
                .icon(IconName::Copy)
                .label("Copy")
                .tooltip("Copy the whole address")
                .on_click(move |_, _, cx| {
                    cx.write_to_clipboard(ClipboardItem::new_string(copied.to_string()));
                }),
        )
}

/// A measured figure on a tile: the number large, what it is under it, small.
pub fn stat(label: &str, value: String, hint: &str, cx: &App) -> Div {
    let theme = cx.theme();
    div()
        .flex()
        .flex_col()
        .gap_1()
        .p_4()
        .min_w(px(160.))
        .flex_1()
        .rounded_xl()
        .border_1()
        .border_color(theme.border)
        .bg(theme.muted)
        .child(
            div()
                .small()
                .text_color(theme.muted_foreground)
                .child(label.to_owned()),
        )
        .child(
            div()
                .text_size(rems(1.5))
                .font_weight(FontWeight::SEMIBOLD)
                .child(value),
        )
        .child(
            div()
                .meta()
                .text_color(theme.muted_foreground)
                .child(hint.to_owned()),
        )
}
