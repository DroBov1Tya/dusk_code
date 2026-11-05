//! The sent messages as a short column of ticks left of the composer. Under the pointer
//! it grows into a catalog: each line shows the message's number and its first words,
//! cut to fit, and a click scrolls the chat to it.

use gpui_kit::{component::ActiveTheme, prelude::FluentBuilder, *};

use crate::workspace::Workspace;

/// How many ticks the folded column shows at most; the catalog shows them all.
const FOLDED: usize = 12;
/// The first words that fit a catalog line.
const PREVIEW: usize = 34;
const WIDE: f32 = 300.;

pub fn render(
    ws: &Workspace,
    sent: &[(usize, String)],
    cx: &mut Context<Workspace>,
) -> Stateful<Div> {
    let theme = cx.theme();
    let (muted, foreground, panel, border, raised) = (
        theme.muted_foreground,
        theme.foreground,
        theme.popover,
        theme.border,
        theme.accent,
    );
    let open = ws.navigator_open;
    let step = sent.len().div_ceil(FOLDED).max(1);
    let mut catalog = div().flex().flex_col().gap_0p5();
    for (number, (row, text)) in sent.iter().enumerate() {
        if !open && number % step != 0 {
            continue;
        }
        let row = *row;
        let first: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
        let mut preview: String = first.chars().take(PREVIEW).collect();
        if first.chars().count() > PREVIEW {
            preview.push('…');
        }
        catalog = catalog.child(
            div()
                .id(("sent", number))
                .group("tick")
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .h(px(if open { 24. } else { 10. }))
                .px_1()
                .rounded_sm()
                .cursor_pointer()
                .when(open, |line| line.hover(move |line| line.bg(raised)))
                .child(
                    div()
                        .flex_none()
                        .w(px(14.))
                        .h(px(2.))
                        .rounded_full()
                        .bg(muted.opacity(0.6))
                        .group_hover("tick", move |tick| tick.bg(foreground)),
                )
                .when(open, |line| {
                    line.child(
                        div()
                            .flex_none()
                            .w(px(22.))
                            .text_xs()
                            .text_color(muted)
                            .child((number + 1).to_string()),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_xs()
                            .text_color(foreground.opacity(0.85))
                            .child(preview),
                    )
                })
                .on_click(cx.listener(move |ws, _, _, cx| {
                    ws.scroll.scroll_to_item(row);
                    ws.navigator_open = false;
                    cx.notify();
                })),
        );
    }
    div()
        .id("navigator")
        // Right beside the composer, growing up and out under the pointer.
        .absolute()
        .right(relative(1.))
        .bottom(px(8.))
        .mr_1()
        .w(px(if open { WIDE } else { 24. }))
        .max_h(px(420.))
        .overflow_y_scroll()
        .p_1()
        .rounded_lg()
        .when(open, |nav| {
            nav.bg(panel).border_1().border_color(border).shadow_lg()
        })
        .on_hover(cx.listener(|ws, hovered: &bool, _, cx| {
            ws.navigator_open = *hovered;
            cx.notify();
        }))
        .child(catalog)
}
