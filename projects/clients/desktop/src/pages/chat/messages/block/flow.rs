//! A flow: layers stacked top to bottom with an arrow between them, the parts of each in
//! a grid. Frameless, it stands in the transcript column. It is text only: no hover, no
//! pointer, no focus stop, so it cannot pass for a control, except the one button of the
//! app that unfolds a tall flow.

use dusk_protocol::thread::{Flow, Layer};
use gpui_kit::{
    component::{
        ActiveTheme, Sizable,
        button::{Button, ButtonVariants},
    },
    *,
};

use super::glyph;
use crate::{ui::Type, workspace::Workspace};

/// Keys of folded things are `(chat, index | KIND)`: this one is for a flow's long tail.
const FOLD: usize = 1 << 43;
/// Padding of a layer, the gap its arrow takes, and a card's least height.
const PAD: f32 = 16.;
const GAP: f32 = glyph::ARROW_HEIGHT;
const CARD: f32 = 52.;
/// A flow taller than this shows its first layers and folds the rest.
const TALL: f32 = 640.;
const SHOWN: usize = 4;

pub fn render(
    ws: &Workspace,
    shown: &str,
    at: usize,
    flow: &Flow,
    cx: &mut Context<Workspace>,
) -> AnyElement {
    let layers = flow.layers();
    let columns = columns(ws.reading_width);
    let open = ws.unfolded.contains(&(shown.to_owned(), at | FOLD));
    let kept = if open || !folds(layers, columns) {
        layers.len()
    } else {
        SHOWN
    };
    let theme = cx.theme();
    let muted = theme.muted_foreground;
    let mut stack = div().flex().flex_col();
    for (index, layer) in layers[..kept].iter().enumerate() {
        if index > 0 {
            stack = stack.child(
                div()
                    .h(px(GAP))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(glyph::arrow(muted)),
            );
        }
        stack = stack.child(layer_card(layer, columns, cx));
    }
    if kept < layers.len() {
        let (chat, count) = (shown.to_owned(), layers.len());
        stack = stack.child(
            div().pt_2().child(
                Button::new(SharedString::from(format!("flow-all-{shown}-{at}")))
                    .ghost()
                    .small()
                    .label(format!("Show all {count} layers"))
                    .on_click(cx.listener(move |ws, _, _, cx| ws.fold(&chat, at | FOLD, cx))),
            ),
        );
    } else if open {
        let chat = shown.to_owned();
        stack = stack.child(
            div().pt_2().child(
                Button::new(SharedString::from(format!("flow-less-{shown}-{at}")))
                    .ghost()
                    .small()
                    .label("Show fewer")
                    .on_click(cx.listener(move |ws, _, _, cx| ws.fold(&chat, at | FOLD, cx))),
            ),
        );
    }
    stack.into_any_element()
}

fn layer_card(layer: &Layer, columns: usize, cx: &App) -> Div {
    let theme = cx.theme();
    let (surface, raised) = (theme.muted, theme.secondary);
    let (muted, accent, hairline) = (theme.muted_foreground, theme.primary, theme.border);
    let cards = layer.cards();
    let mut card = div()
        .flex()
        .flex_col()
        .p(px(PAD))
        .rounded(px(12.))
        .border_1()
        // The one emphasised layer: its border is the accent, softened. Nothing else changes.
        .border_color(if layer.emphasis {
            accent.opacity(0.45)
        } else {
            hairline
        })
        .bg(surface)
        .child(
            div()
                .body()
                .font_weight(FontWeight::MEDIUM)
                .child(layer.title.to_string()),
        );
    if let Some(subtitle) = &layer.subtitle {
        card = card.child(
            div()
                .min_w_0()
                .truncate()
                .small()
                .text_color(muted)
                .child(subtitle.to_string()),
        );
    }
    if !cards.is_empty() {
        card = card.child(
            div()
                .grid()
                .grid_cols(columns.min(cards.len()) as u16)
                .gap_2()
                .mt_3()
                .children(cards.iter().map(|part| {
                    div()
                        .min_w_0()
                        .min_h(px(CARD))
                        .px_3()
                        .py_2()
                        .rounded_lg()
                        .bg(raised)
                        .child(
                            div()
                                .min_w_0()
                                .truncate()
                                .small()
                                .font_weight(FontWeight::MEDIUM)
                                .child(part.title.to_string()),
                        )
                        .children(part.line.as_ref().map(|line| {
                            div()
                                .meta()
                                .line_height(relative(1.4))
                                .line_clamp(2)
                                .text_color(muted)
                                .child(line.to_string())
                        }))
                })),
        );
    }
    card
}

/// How many cards a row of a layer holds at most, by the width the flow has: 4 from 720,
/// 3 from 540, 2 from 360, else 1.
pub fn columns(width: f32) -> usize {
    match width {
        w if w >= 720. => 4,
        w if w >= 540. => 3,
        w if w >= 360. => 2,
        _ => 1,
    }
}

/// Whether the flow is tall enough, past four layers, to fold its tail.
fn folds(layers: &[Layer], columns: usize) -> bool {
    layers.len() > SHOWN && height(layers, columns) > TALL
}

/// About how tall the flow stands: a layer's padding and head, its rows of cards, and the
/// arrows between. Only to decide on folding, so the head is a fixed guess.
fn height(layers: &[Layer], columns: usize) -> f32 {
    let head = 24.;
    let body: f32 = layers
        .iter()
        .map(|layer| {
            let cards = layer.cards().len();
            let rows = cards.div_ceil(columns.min(cards.max(1)));
            let subtitle = if layer.subtitle.is_some() { 20. } else { 0. };
            let grid = if rows == 0 {
                0.
            } else {
                12. + rows as f32 * CARD + (rows - 1) as f32 * 8.
            };
            2. * PAD + head + subtitle + grid
        })
        .sum();
    body + GAP * layers.len().saturating_sub(1) as f32
}

#[cfg(test)]
mod tests {
    use super::{columns, folds};
    use dusk_protocol::thread::Flow;

    fn flow(layers: usize, cards: usize) -> Flow {
        let card = |n: usize| serde_json::json!({ "title": format!("c{n}") });
        let layer = |n: usize| serde_json::json!({ "title": format!("l{n}"), "cards": (0..cards).map(card).collect::<Vec<_>>() });
        serde_json::from_value(
            serde_json::json!({ "flow": { "layers": (0..layers).map(layer).collect::<Vec<_>>() } }),
        )
        .map(|block: dusk_protocol::thread::Block| match block {
            dusk_protocol::thread::Block::Flow(flow) => flow,
            _ => unreachable!("a flow was asked for"),
        })
        .unwrap()
    }

    #[test]
    fn columns_follow_the_width_in_steps() {
        assert_eq!(
            [800., 720., 719., 540., 539., 360., 359., 0.].map(columns),
            [4, 4, 3, 3, 2, 2, 1, 1]
        );
    }

    #[test]
    fn only_a_tall_flow_of_more_than_four_layers_folds() {
        // Four layers never fold, however tall; many empty layers are short.
        assert!(!folds(flow(4, 8).layers(), 1));
        assert!(!folds(flow(5, 0).layers(), 4));
        assert!(folds(flow(8, 4).layers(), 4));
        assert!(folds(flow(6, 8).layers(), 2));
    }
}
