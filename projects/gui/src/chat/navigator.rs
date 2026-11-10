//! The sent messages, beside the composer: four short ticks while folded, never taller
//! than the composer. Under the pointer a catalog opens to the right, over the chat:
//! each line has the message's number and first words cut to fit, and a click scrolls
//! the chat to it.

use gpui_kit::{component::ActiveTheme, prelude::FluentBuilder, *};

use crate::workspace::Workspace;

const TICKS: usize = 4;
/// The first words that fit a catalog line.
const PREVIEW: usize = 40;

pub fn render(
    ws: &Workspace,
    sent: &[(usize, String)],
    cx: &mut Context<Workspace>,
) -> Stateful<Div> {
    let theme = cx.theme();
    let (muted, foreground, mut panel, border, raised) = (
        theme.muted_foreground,
        theme.foreground,
        theme.popover,
        theme.border,
        theme.accent,
    );
    // Opaque, so the chat behind does not show through the list.
    panel.a = 1.0;
    let mut ticks = div().flex().flex_col().gap_1p5().py_1();
    for _ in 0..TICKS.min(sent.len()) {
        ticks = ticks.child(
            div()
                .w(px(14.))
                .h(px(2.))
                .rounded_full()
                .bg(muted.opacity(if ws.navigator_open { 1. } else { 0.6 })),
        );
    }
    let catalog = ws.navigator_open.then(|| {
        let mut lines = div().flex().flex_col().gap_0p5();
        for (number, (row, text)) in sent.iter().enumerate() {
            let row = *row;
            let words: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
            let mut preview: String = words.chars().take(PREVIEW).collect();
            if words.chars().count() > PREVIEW {
                preview.push('…');
            }
            lines = lines.child(
                div()
                    .id(("sent", number))
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .h(px(26.))
                    .px_2()
                    .rounded_md()
                    .cursor_pointer()
                    .hover(move |line| line.bg(raised))
                    .child(
                        div()
                            .flex_none()
                            .w(px(24.))
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
                            .text_color(foreground.opacity(0.9))
                            .child(preview),
                    )
                    .on_click(cx.listener(move |ws, _, _, cx| {
                        ws.list.scroll_to_reveal_item(row);
                        ws.navigator_open = false;
                        cx.notify();
                    })),
            );
        }
        // Its own window, opening over the chat to the right of the ticks.
        div()
            .id("navigator-catalog")
            .absolute()
            .left_0()
            .bottom_0()
            .w(px(320.))
            .max_h(px(360.))
            .overflow_y_scroll()
            .p_1()
            .rounded_xl()
            .border_1()
            .border_color(border)
            .bg(panel)
            .shadow_lg()
            .on_hover(cx.listener(|ws, hovered: &bool, _, cx| {
                ws.navigator_open = *hovered;
                cx.notify();
            }))
            .child(lines)
    });
    div()
        .id("navigator")
        .absolute()
        .right(relative(1.))
        .bottom(px(12.))
        .mr_2()
        .p_1()
        .cursor_pointer()
        .on_hover(cx.listener(|ws, hovered: &bool, _, cx| {
            if *hovered {
                ws.navigator_open = true;
                cx.notify();
            }
        }))
        .child(ticks)
        .when_some(catalog, |nav, catalog| nav.child(catalog))
}
