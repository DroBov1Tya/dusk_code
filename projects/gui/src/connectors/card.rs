//! A card of the connectors page: an icon slot, the name, what kind it is, a line of
//! description, its state and its action.

use dusk_protocol::thread::fnv1a;
use gpui_kit::{component::ActiveTheme, *};

/// Hues the letter tiles take, so neighbours differ.
const HUES: [f32; 8] = [0.0, 0.08, 0.15, 0.33, 0.5, 0.6, 0.72, 0.85];

pub struct Card {
    pub id: SharedString,
    pub name: String,
    /// Such as `MCP · local`.
    pub kind: String,
    pub description: String,
    pub state: Option<(String, Hsla)>,
    /// A picture the core found for it; a letter tile otherwise.
    pub icon: Option<std::path::PathBuf>,
}

pub fn render(card: Card, action: impl IntoElement, cx: &App) -> Stateful<Div> {
    let theme = cx.theme();
    div()
        .id(card.id)
        .flex()
        .flex_row()
        .gap_3()
        .p_3()
        .min_h(px(92.))
        .rounded_lg()
        .border_1()
        .border_color(theme.border)
        .bg(theme.muted)
        .cursor_pointer()
        .hover(|card| card.border_color(theme.muted_foreground.opacity(0.4)))
        .child(match card.icon {
            Some(path) => div()
                .size(px(44.))
                .flex_none()
                .rounded_lg()
                .overflow_hidden()
                .child(img(path).size_full()),
            None => icon_slot(&card.name, cx),
        })
        .child(
            div()
                .flex()
                .flex_col()
                .gap_0p5()
                .flex_1()
                .min_w_0()
                .child(
                    div()
                        .font_weight(FontWeight::SEMIBOLD)
                        .truncate()
                        .child(card.name),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(card.kind),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .line_clamp(2)
                        .text_ellipsis()
                        .child(card.description),
                ),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .items_end()
                .justify_between()
                .gap_2()
                .children(
                    card.state
                        .map(|(text, color)| div().text_xs().text_color(color).child(text)),
                )
                // The action keeps its clicks, so a switch does not open the card.
                .child(
                    div()
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .child(action),
                ),
        )
}

/// Where a connector's own icon goes once the core gives one; until then, its first
/// letter on a tile whose color comes from its name.
pub fn icon_slot(name: &str, cx: &App) -> Div {
    let hue = HUES[(fnv1a(name.as_bytes()) % HUES.len() as u64) as usize];
    let dark = cx.theme().is_dark();
    let tile = hsla(hue, 0.45, if dark { 0.3 } else { 0.85 }, 1.);
    let letter = name
        .chars()
        .find(|c| c.is_alphanumeric())
        .map(|c| c.to_uppercase().to_string())
        .unwrap_or_else(|| "?".to_owned());
    div()
        .size(px(44.))
        .flex_none()
        .rounded_lg()
        .bg(tile)
        .flex()
        .items_center()
        .justify_center()
        .text_lg()
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(cx.theme().foreground)
        .child(letter)
}
