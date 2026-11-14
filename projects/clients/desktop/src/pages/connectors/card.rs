//! A card of the connectors page: an icon slot, the name, what kind it is, a line of
//! description, its state and its action.

use crate::ui::{Type, widgets::Focusable};
use gpui_kit::{component::ActiveTheme, *};

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
        .focusable_row(cx)
        .w(px(360.))
        .max_w_full()
        .flex()
        .flex_row()
        .gap_3()
        .p_3()
        .min_h(px(84.))
        .rounded_xl()
        .border_1()
        .border_color(theme.border)
        .bg(theme.muted)
        .cursor_pointer()
        .hover(|card| card.border_color(theme.muted_foreground.opacity(0.4)))
        .child(match card.icon {
            Some(path) => div()
                .size(px(36.))
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
                        .body()
                        .font_weight(FontWeight::SEMIBOLD)
                        .truncate()
                        .child(card.name),
                )
                .child(
                    div()
                        .meta()
                        .text_color(theme.muted_foreground)
                        .child(card.kind),
                )
                .child(
                    div()
                        .small()
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
                        .map(|(text, color)| div().meta().text_color(color).child(text)),
                )
                // The action keeps its clicks, so a switch does not open the card.
                .child(
                    div()
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .child(action),
                ),
        )
}

/// Keep the fallback quiet so each connector's status gets the color.
pub fn icon_slot(name: &str, cx: &App) -> Div {
    let letter = name
        .chars()
        .find(|c| c.is_alphanumeric())
        .map(|c| c.to_uppercase().to_string())
        .unwrap_or_else(|| "?".to_owned());
    div()
        .size(px(34.))
        .flex_none()
        .rounded_md()
        .bg(cx.theme().accent)
        .flex()
        .items_center()
        .justify_center()
        .small()
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(cx.theme().foreground)
        .child(letter)
}
