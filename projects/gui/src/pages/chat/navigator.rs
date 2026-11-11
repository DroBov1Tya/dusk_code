//! A compact index of the user's sent messages, beside the composer.

use std::cell::Cell;

use gpui_kit::{
    component::{ActiveTheme, Colorize},
    prelude::FluentBuilder,
    *,
};

use crate::{
    ui::{
        Type,
        widgets::{self, Focusable},
    },
    workspace::Workspace,
};

const TICKS: usize = 4;
const ROW: f32 = 32.;
const GAP: f32 = 2.;

pub fn render(
    ws: &Workspace,
    sent: &[(usize, String)],
    window: &mut Window,
    cx: &mut Context<Workspace>,
) -> Stateful<Div> {
    let theme = cx.theme();
    let (foreground, mut panel, border, raised, accent) = (
        theme.foreground,
        theme.popover,
        theme.border,
        theme.list_hover,
        theme.primary,
    );
    let muted = foreground.mix_oklab(theme.muted_foreground, 0.16);
    let selected = theme.list_active;
    panel.a = 1.0;
    let open = ws.navigator_open;
    let mut ticks = div().flex().flex_col().gap_1().items_center();
    for index in 0..TICKS.min(sent.len()) {
        ticks = ticks.child(
            div()
                .w(px(if index == 0 { 13. } else { 9. }))
                .h(px(2.))
                .rounded_full()
                .bg(if open { accent } else { muted }),
        );
    }
    let catalog = open.then(|| {
        let viewport = window.viewport_size();
        let width = px((viewport.width.as_f32() - 24.).clamp(160., 344.));
        let available = (viewport.height.as_f32() - 170.).clamp(ROW, 308.);
        let visible_rows = ((available + GAP) / (ROW + GAP)).floor();
        let list_height = px(visible_rows * (ROW + GAP) - GAP);
        let top = ws.list.logical_scroll_top().item_ix;
        let current = if ws.list.is_following_tail() {
            sent.len() - 1
        } else {
            sent.partition_point(|(row, _)| *row <= top)
                .saturating_sub(1)
        };
        let position = window.use_keyed_state("navigator-position", cx, |_, _| {
            (ScrollHandle::new(), Cell::new(None))
        });
        let (scroll, previous) = position.read(cx);
        if previous.replace(Some(current)) != Some(current) {
            let offset = (px((current as f32 + 0.5) * (ROW + GAP)) - list_height / 2.).max(px(0.));
            scroll.set_offset(point(px(0.), -offset));
        }
        let mut lines = div()
            .id("navigator-scroll")
            .track_scroll(scroll)
            .max_h(list_height)
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap(px(GAP));
        for (number, (row, text)) in sent.iter().enumerate() {
            let row = *row;
            let words = super::messages::label(text);
            let preview = if words.is_empty() {
                "Message".to_owned()
            } else if let Some((end, _)) = words.char_indices().nth(120) {
                format!("{}…", &words[..end])
            } else {
                words
            };
            let active = number == current;
            lines = lines.child(
                div()
                    .id(("sent", row))
                    .focusable_row(cx)
                    .role(Role::Button)
                    .aria_label(format!("Message {}: {preview}", number + 1))
                    .when(active, |line| {
                        line.aria_description("Current position").bg(selected)
                    })
                    .flex()
                    .items_center()
                    .gap_2()
                    .h(px(ROW))
                    .flex_none()
                    .px_2()
                    .rounded_md()
                    .cursor_pointer()
                    .hover(move |line| line.bg(raised))
                    .child(
                        div()
                            .flex_none()
                            .min_w(px(28.))
                            .meta()
                            .text_color(if active { accent } else { muted })
                            .child((number + 1).to_string()),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .small()
                            .text_color(foreground)
                            .child(preview),
                    )
                    .on_click(cx.listener(move |ws, _, _, cx| {
                        ws.list.scroll_to_reveal_item(row);
                        ws.navigator_open = false;
                        cx.notify();
                    })),
            );
        }
        let body = div()
            .id("navigator-catalog")
            .w(width)
            .flex()
            .flex_col()
            .gap_1()
            .p_2()
            .rounded_xl()
            .border_1()
            .border_color(border)
            .bg(panel)
            .shadow_lg()
            .occlude()
            .on_hover(cx.listener(|ws, hovered: &bool, _, cx| {
                if !*hovered {
                    ws.navigator_open = false;
                    cx.notify();
                }
            }))
            .on_mouse_down_out(cx.listener(|ws, _, _, cx| {
                ws.navigator_open = false;
                cx.notify();
            }))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_2()
                    .py_1()
                    .child(div().group_name().flex_1().child("Message history"))
                    .child(div().meta().text_color(muted).child(format!(
                        "{} of {}",
                        current + 1,
                        sent.len()
                    ))),
            )
            .child(lines);
        div().absolute().left_0().bottom_0().child(widgets::on_top(
            anchored()
                .anchor(Anchor::BottomLeft)
                .snap_to_window_with_margin(Edges::all(px(12.)))
                .child(widgets::appear("navigator-list", body)),
        ))
    });
    div()
        .id("navigator")
        .absolute()
        .right(relative(1.))
        .bottom(px(12.))
        .mr_1()
        .child(
            div()
                .id("navigator-trigger")
                .focusable_row(cx)
                .role(Role::Button)
                .aria_label("Message history")
                .aria_expanded(open)
                .size(px(22.))
                .flex()
                .items_center()
                .justify_center()
                .rounded_md()
                .cursor_pointer()
                .hover(move |button| button.bg(raised))
                .on_hover(cx.listener(|ws, hovered: &bool, _, cx| {
                    if *hovered {
                        ws.navigator_open = true;
                        cx.notify();
                    }
                }))
                .on_click(cx.listener(|ws, _, _, cx| {
                    ws.navigator_open = true;
                    cx.notify();
                }))
                .child(ticks),
        )
        .children(catalog)
}
