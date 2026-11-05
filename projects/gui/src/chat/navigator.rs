//! The sent messages as a short column of ticks left of the composer. Under the pointer it
//! grows into a catalog: each tick shows its message's number and first words, and a
//! click scrolls the chat to it.

use gpui_kit::{component::ActiveTheme, *};

use crate::workspace::Workspace;

/// How many ticks the folded column shows at most; the catalog shows them all.
const FOLDED: usize = 12;
const PREVIEW: usize = 48;

pub fn render(sent: &[(usize, String)], cx: &mut Context<Workspace>) -> Stateful<Div> {
    let theme = cx.theme();
    let (muted, foreground, panel, border) = (
        theme.muted_foreground,
        theme.foreground,
        theme.popover,
        theme.border,
    );
    let mut catalog = div().flex().flex_col().gap_0p5();
    let step = sent.len().div_ceil(FOLDED).max(1);
    for (number, (row, text)) in sent.iter().enumerate() {
        let row = *row;
        let preview: String = text
            .lines()
            .next()
            .unwrap_or_default()
            .chars()
            .take(PREVIEW)
            .collect();
        // Folded, only every few ticks show, so a long chat stays a short column.
        let folded_visible = number % step == 0;
        catalog = catalog.child(
            div()
                .id(("sent", number))
                .group("tick")
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .h(px(14.))
                .cursor_pointer()
                .when_folded(!folded_visible)
                .child(
                    div()
                        .w(px(14.))
                        .h(px(2.))
                        .rounded_full()
                        .bg(muted.opacity(0.6))
                        .group_hover("tick", move |tick| tick.bg(foreground).w(px(20.))),
                )
                .child(
                    div()
                        .invisible()
                        .group_hover("navigator", |label| label.visible())
                        .text_xs()
                        .text_color(muted)
                        .group_hover("tick", move |label| label.text_color(foreground))
                        .truncate()
                        .child(format!("{}  {preview}", number + 1)),
                )
                .on_click(cx.listener(move |ws, _, _, cx| {
                    ws.scroll.scroll_to_item(row);
                    cx.notify();
                })),
        );
    }
    div()
        .id("navigator")
        .group("navigator")
        // Right beside the composer, growing up and out under the pointer.
        .absolute()
        .right(relative(1.))
        .bottom(px(8.))
        .mr_1()
        .w(px(28.))
        .max_h(relative(0.8))
        .overflow_y_scroll()
        .p_1()
        .rounded_lg()
        .hover(move |nav| nav.w(px(280.)).bg(panel).border_1().border_color(border))
        .child(catalog)
}

trait WhenFolded {
    fn when_folded(self, hidden: bool) -> Self;
}

impl WhenFolded for Stateful<Div> {
    /// A tick hidden in the folded column comes back when the catalog opens.
    fn when_folded(self, hidden: bool) -> Self {
        if hidden {
            self.h(px(0.))
                .overflow_hidden()
                .group_hover("navigator", |tick| tick.h(px(14.)))
        } else {
            self
        }
    }
}
