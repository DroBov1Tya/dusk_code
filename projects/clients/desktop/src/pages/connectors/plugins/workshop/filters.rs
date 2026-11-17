//! The filter row: a segmented switch between category, tier and kind chips, and the
//! order of the list. A category and an order are the core's to apply, so choosing one
//! reads the store again; tier and kind only narrow what is shown.

use dusk_protocol::plugin::{Kind, Order, Tier};
use gpui_kit::{
    component::{ActiveTheme, Sizable, button::Button},
    prelude::FluentBuilder,
    *,
};

use super::tier;
use crate::{ui::controls::choice, workspace::Workspace};

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum Facet {
    #[default]
    Category,
    Tier,
    Kind,
}

/// Category chips shown before "+N".
const CHIPS: usize = 5;

const ORDERS: [(Order, &str); 4] = [
    (Order::Trust, "Official first"),
    (Order::Newest, "Newest"),
    (Order::Updated, "Recently updated"),
    (Order::Stars, "Most stars"),
];

const KINDS: [Kind; 4] = [Kind::Plugin, Kind::Skill, Kind::Pack, Kind::Agent];

pub fn order_word(order: Order) -> &'static str {
    ORDERS
        .iter()
        .find(|(each, _)| *each == order)
        .map_or("", |(_, word)| word)
}

pub fn render(ws: &Workspace, cx: &mut Context<Workspace>) -> Div {
    let shop = &ws.workshop;
    let chips = match shop.facet {
        Facet::Category => category_chips(ws, cx),
        Facet::Tier => tier_chips(ws, cx),
        Facet::Kind => kind_chips(ws, cx),
    };
    let mut orders = div().flex().flex_row().items_center().gap_0p5();
    for (order, label) in ORDERS {
        orders = orders.child(
            choice(
                Button::new(SharedString::from(format!("workshop-order-{label}"))).small(),
                shop.order == order,
                cx,
            )
            .label(label)
            .on_click(cx.listener(move |ws, _, _, cx| {
                if ws.workshop.order != order {
                    ws.workshop.order = order;
                    ws.load_store(cx);
                }
            })),
        );
    }
    div()
        .flex()
        .flex_col()
        .gap_3()
        .child(
            div()
                .flex()
                .flex_row()
                .flex_wrap()
                .items_center()
                .gap_3()
                .child(segments(shop.facet, cx))
                .child(chips),
        )
        .child(orders)
}

/// "Category | Tier | Kind", one always chosen.
fn segments(facet: Facet, cx: &mut Context<Workspace>) -> Div {
    let theme = cx.theme();
    let (foreground, muted, raised, surface) = (
        theme.foreground,
        theme.muted_foreground,
        theme.background,
        theme.muted,
    );
    let mut row = div()
        .flex()
        .flex_row()
        .gap_0p5()
        .p_0p5()
        .rounded_lg()
        .bg(surface);
    for (each, label) in [
        (Facet::Category, "Category"),
        (Facet::Tier, "Tier"),
        (Facet::Kind, "Kind"),
    ] {
        let active = facet == each;
        row = row.child(
            div()
                .id(SharedString::from(format!("workshop-facet-{label}")))
                .px_3()
                .py_0p5()
                .rounded_md()
                .cursor_pointer()
                .text_sm()
                .font_weight(FontWeight::MEDIUM)
                .when(active, |tab| tab.bg(raised))
                .text_color(if active { foreground } else { muted })
                .child(label)
                .on_click(cx.listener(move |ws, _, _, cx| {
                    ws.workshop.facet = each;
                    cx.notify();
                })),
        );
    }
    row
}

fn chip(id: String, label: String, chosen: bool, cx: &App) -> Button {
    choice(Button::new(SharedString::from(id)).small(), chosen, cx).label(label)
}

fn chip_row() -> Div {
    div().flex().flex_row().flex_wrap().items_center().gap_1()
}

fn category_chips(ws: &Workspace, cx: &mut Context<Workspace>) -> Div {
    let shop = &ws.workshop;
    let mut row = chip_row().child(
        chip(
            "workshop-cat-all".into(),
            "All".into(),
            shop.category.is_none(),
            cx,
        )
        .on_click(cx.listener(|ws, _, _, cx| {
            if ws.workshop.category.take().is_some() {
                ws.load_store(cx);
            }
        })),
    );
    let shown = if shop.more_chips {
        shop.categories.len()
    } else {
        CHIPS
    };
    for category in shop.categories.iter().take(shown) {
        let chosen = shop.category.as_deref() == Some(category);
        let picked = category.clone();
        row = row.child(
            chip(
                format!("workshop-cat-{category}"),
                crate::ui::widgets::capitalize(category),
                chosen,
                cx,
            )
            .on_click(cx.listener(move |ws, _, _, cx| {
                if ws.workshop.category.as_ref() != Some(&picked) {
                    ws.workshop.category = Some(picked.clone());
                    ws.load_store(cx);
                }
            })),
        );
    }
    let hidden = shop.categories.len().saturating_sub(CHIPS);
    if hidden > 0 {
        let label = if shop.more_chips {
            "Fewer".to_owned()
        } else {
            format!("+{hidden}")
        };
        row = row.child(
            chip("workshop-cat-more".into(), label, false, cx).on_click(cx.listener(
                |ws, _, _, cx| {
                    ws.workshop.more_chips = !ws.workshop.more_chips;
                    cx.notify();
                },
            )),
        );
    }
    row
}

fn tier_chips(ws: &Workspace, cx: &mut Context<Workspace>) -> Div {
    let current = ws.workshop.tier;
    let mut row = chip_row().child(
        chip(
            "workshop-tier-all".into(),
            "All".into(),
            current.is_none(),
            cx,
        )
        .on_click(cx.listener(|ws, _, _, cx| {
            ws.workshop.tier = None;
            cx.notify();
        })),
    );
    for each in tier::ALL {
        row = row.child(tier_chip(each, current == Some(each), cx));
    }
    row
}

/// A tier's chip carries its mark, so the filter and the sections read the same.
fn tier_chip(each: Tier, chosen: bool, cx: &mut Context<Workspace>) -> Button {
    choice(
        Button::new(SharedString::from(format!(
            "workshop-tier-{}",
            tier::word(each)
        )))
        .small(),
        chosen,
        cx,
    )
    .child(tier::mark(each, cx))
    .on_click(cx.listener(move |ws, _, _, cx| {
        ws.workshop.tier = Some(each);
        cx.notify();
    }))
}

fn kind_chips(ws: &Workspace, cx: &mut Context<Workspace>) -> Div {
    let current = ws.workshop.kind;
    let mut row = chip_row().child(
        chip(
            "workshop-kind-all".into(),
            "All".into(),
            current.is_none(),
            cx,
        )
        .on_click(cx.listener(|ws, _, _, cx| {
            ws.workshop.kind = None;
            cx.notify();
        })),
    );
    for each in KINDS {
        let word = super::kind_word(each);
        row = row.child(
            chip(
                format!("workshop-kind-{word}"),
                format!("{word}s"),
                current == Some(each),
                cx,
            )
            .on_click(cx.listener(move |ws, _, _, cx| {
                ws.workshop.kind = Some(each);
                cx.notify();
            })),
        );
    }
    row
}
