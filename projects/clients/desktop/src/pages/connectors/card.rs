//! A row of the connectors page, in a card of rows: an icon slot, the name over one line
//! of what kind it is and what it does, its state and its actions at the right end.

use crate::ui::{Type, widgets::Focusable};
use gpui_kit::{component::ActiveTheme, *};

/// A row's height: two lines of text and their padding.
const HEIGHT: f32 = 68.;
const ICON: f32 = 36.;
/// One line is ever shown; the rest is not shaped.
const LINE_CHARS: usize = 300;

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
    let (muted, hover) = (theme.muted_foreground, theme.list_hover);
    let facts = match one_line(&card.description) {
        about if about.is_empty() => card.kind,
        about => format!("{} \u{b7} {about}", card.kind),
    };
    div()
        .id(card.id)
        .focusable_row(cx)
        .flex()
        .flex_row()
        .items_center()
        .gap_3()
        .px_3()
        .h(px(HEIGHT))
        .rounded_lg()
        .cursor_pointer()
        .hover(move |row| row.bg(hover))
        .child(match card.icon {
            Some(path) => div()
                .size(px(ICON))
                .flex_none()
                .child(img(path).size_full().rounded_lg()),
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
                .child(div().small().text_color(muted).truncate().child(facts)),
        )
        .children(card.state.map(|(text, color)| {
            div()
                .flex_none()
                .meta()
                .font_weight(FontWeight::MEDIUM)
                .text_color(color)
                .child(text)
        }))
        // The action keeps its clicks, so a switch does not open the row's window.
        .child(
            div()
                .flex_none()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(action),
        )
}

/// Text as one plain line of at most `LINE_CHARS` characters.
fn one_line(text: &str) -> String {
    let mut line: String = text
        .split_whitespace()
        .flat_map(|word| std::iter::once(' ').chain(word.chars()))
        .skip(1)
        .take(LINE_CHARS)
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    if line.chars().count() == LINE_CHARS {
        line.push('\u{2026}');
    }
    line
}

/// Keep the fallback quiet so each connector's status gets the color.
pub fn icon_slot(name: &str, cx: &App) -> Div {
    let letter = name
        .chars()
        .find(|c| c.is_alphanumeric())
        .map(|c| c.to_uppercase().to_string())
        .unwrap_or_else(|| "?".to_owned());
    div()
        .size(px(ICON))
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
