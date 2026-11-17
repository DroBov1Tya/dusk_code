//! Discover: the official listings as a carousel, one large card at a time, with its
//! cover on the right, dots for where it is and arrows to the next.

use dusk_protocol::plugin::{Listing, Tier};
use gpui_kit::{
    component::{
        ActiveTheme, Icon, IconName, Sizable,
        button::{Button, ButtonVariants},
    },
    prelude::FluentBuilder,
    *,
};

use super::card;
use crate::{ui::Type, workspace::Workspace};

const PICKS: usize = 5;

/// The first official listings, in the core's order.
pub fn featured<'a>(shown: &[&'a Listing]) -> Vec<&'a Listing> {
    shown
        .iter()
        .copied()
        .filter(|listing| listing.tier == Tier::Official)
        .take(PICKS)
        .collect()
}

pub fn render(ws: &Workspace, picks: &[&Listing], cx: &mut Context<Workspace>) -> Div {
    let theme = cx.theme();
    let (border, muted, surface, accent) = (
        theme.border,
        theme.muted_foreground,
        theme.muted,
        theme.primary,
    );
    let count = picks.len();
    let index = ws.workshop.discover % count;
    let listing = picks[index];
    let busy = ws.workshop.busy.get(&listing.name).copied();
    let source = match listing.source.as_str() {
        "" => "duskcode".to_owned(),
        source => source.to_owned(),
    };
    let text = div()
        .flex()
        .flex_col()
        .gap_2()
        .flex_1()
        .min_w_0()
        .child(
            div()
                .meta()
                .text_color(muted)
                .child(format!("From {source}")),
        )
        .child(
            div()
                .text_size(rems(2.1))
                .line_height(rems(2.4))
                .font_weight(FontWeight::BOLD)
                .truncate()
                .child(listing.name.clone()),
        )
        .child(
            div()
                .body()
                .text_color(muted)
                .line_clamp(2)
                .text_ellipsis()
                .child(listing.description.clone()),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .flex_wrap()
                .items_center()
                .gap_3()
                .child(card::facts(listing, cx))
                .child(
                    div()
                        .px_2()
                        .rounded_md()
                        .border_1()
                        .border_color(border)
                        .meta()
                        .text_color(muted)
                        .child(format!("v{}", listing.version)),
                ),
        )
        .child(
            div()
                .pt_2()
                .flex()
                .flex_row()
                .gap_1()
                .children(card::actions(listing, busy, cx)),
        );
    let cover = match ws.workshop.covers.get(listing) {
        Some(image) => div()
            .w(px(300.))
            .h(px(180.))
            .flex_none()
            .rounded_xl()
            .overflow_hidden()
            .child(img(image).size_full().object_fit(ObjectFit::Cover)),
        None => div()
            .w(px(300.))
            .h(px(180.))
            .flex_none()
            .rounded_xl()
            .bg(accent.opacity(0.12))
            .flex()
            .items_center()
            .justify_center()
            .child(super::tier::icon(listing.tier, 56., cx)),
    };
    let body = div()
        .flex()
        .flex_row()
        .items_center()
        .gap_6()
        .child(text)
        .child(cover);
    let dots = div()
        .flex()
        .flex_row()
        .items_center()
        .gap_1p5()
        .children((0..count).map(|dot| {
            div()
                .id(("discover-dot", dot))
                .size(px(7.))
                .rounded_full()
                .cursor_pointer()
                .bg(if dot == index {
                    accent
                } else {
                    muted.opacity(0.35)
                })
                .on_click(cx.listener(move |ws, _, _, cx| {
                    ws.workshop.discover = dot;
                    cx.notify();
                }))
        }));
    let arrow = |id: &'static str, icon: IconName, label: &'static str, step: usize| {
        Button::new(id)
            .ghost()
            .small()
            .icon(Icon::new(icon))
            .tooltip(label)
            .on_click(cx.listener(move |ws, _, _, cx| {
                ws.workshop.discover = (ws.workshop.discover % count + step) % count;
                cx.notify();
            }))
    };
    div()
        .flex()
        .flex_col()
        .gap_3()
        .p_6()
        .rounded_2xl()
        .border_1()
        .border_color(border)
        .bg(surface)
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .child(
                    div()
                        .small()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child("Discover"),
                )
                .child(div().flex_1())
                .when(count > 1, |row| {
                    row.child(arrow(
                        "discover-back",
                        IconName::ChevronLeft,
                        "Previous",
                        count - 1,
                    ))
                    .child(arrow(
                        "discover-next",
                        IconName::ChevronRight,
                        "Next",
                        1,
                    ))
                }),
        )
        // Each pick fades in as it comes, so the change is seen without a slide.
        .child(crate::ui::widgets::appear(("discover", index), body))
        .when(count > 1, |card| {
            card.child(div().flex().justify_center().child(dots))
        })
}
