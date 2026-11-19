//! Room for the buttons in a code block's corner. The library draws them over the
//! block, so a long line runs under them unless the block keeps its right side free.
//! The library's own text style is rebuilt as the component theme builds it (same
//! fields, `TextViewDefaults` of the component keeps the highlighter and the
//! inherited text color) with one change: the code block's right padding.

use gpui_kit::{
    base::{TextViewDefaults, TextViewStyle},
    component::{ActiveTheme, Theme},
    *,
};

/// Two small buttons, the gap between them, and the corner offset they sit at, plus
/// air before the text. A plain block has one button; the width is the same for all.
const RIGHT: Pixels = px(68.);

pub fn install(cx: &mut App) {
    let style = style(cx.theme());
    TextViewDefaults::global(cx).with_style(style).install(cx);
}

fn style(theme: &Theme) -> TextViewStyle {
    let radius = theme.semantic_tokens().radius.md;
    let mut table = StyleRefinement::default();
    table.corner_radii.top_left = Some(radius.into());
    table.corner_radii.top_right = Some(radius.into());
    table.corner_radii.bottom_left = Some(radius.into());
    table.corner_radii.bottom_right = Some(radius.into());
    let mut code_block = StyleRefinement::default().pr(RIGHT);
    code_block.corner_radii = table.corner_radii.clone();
    let table_head = StyleRefinement::default()
        .bg(theme.table_head)
        .text_color(theme.table_head_foreground);

    TextViewStyle::default()
        .with_foreground(theme.foreground)
        .with_muted_foreground(theme.muted_foreground)
        .with_link(theme.link)
        .with_selection(theme.selection)
        .with_code_background(theme.muted)
        .with_border(theme.border)
        .with_code_block(code_block)
        .with_table(table)
        .with_table_head(table_head)
        .with_inline_code(HighlightStyle {
            background_color: Some(theme.accent),
            ..Default::default()
        })
        .with_dark(theme.is_dark())
}
