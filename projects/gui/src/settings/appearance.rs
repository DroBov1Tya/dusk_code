//! How the window looks: the theme, and the fonts with their sizes.

use gpui_kit::{
    component::{
        ActiveTheme, Colorize, Sizable,
        button::{Button, ButtonVariants},
        input::Input,
    },
    *,
};

use super::line;
use crate::{theme, widgets, workspace::Workspace};

pub fn render(ws: &mut Workspace, _: &mut Window, cx: &mut Context<Workspace>) -> Div {
    let prefs = theme::prefs(cx).clone();
    let mut themes = div().grid().grid_cols(3).gap_3().pt_2();
    for palette in theme::PALETTES {
        let name = palette.name;
        let chosen = name == prefs.theme;
        let swatch = |color: &str| div().size_3().rounded_full().bg(hex(color));
        themes = themes.child(
            div()
                .id(SharedString::from(format!("theme-{name}")))
                .flex()
                .flex_col()
                .gap_3()
                .p_3()
                .rounded_lg()
                .border_1()
                .border_color(if chosen {
                    hex(palette.accent)
                } else {
                    cx.theme().border
                })
                .bg(hex(palette.background))
                .cursor_pointer()
                .child(
                    div()
                        .h(px(44.))
                        .rounded_md()
                        .bg(hex(palette.panel))
                        .flex()
                        .flex_col()
                        .gap_1()
                        .p_2()
                        .child(
                            div()
                                .h(px(6.))
                                .w(relative(0.6))
                                .rounded_sm()
                                .bg(hex(palette.muted)),
                        )
                        .child(
                            div()
                                .h(px(6.))
                                .w(relative(0.4))
                                .rounded_sm()
                                .bg(hex(palette.accent)),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_1()
                        .child(
                            div()
                                .flex_1()
                                .text_sm()
                                .text_color(hex(palette.foreground))
                                .child(name),
                        )
                        .child(swatch(palette.surface))
                        .child(swatch(palette.accent)),
                )
                .on_click(cx.listener(move |_, _, _, cx| {
                    theme::change(cx, |prefs| prefs.theme = name.to_owned());
                })),
        );
    }
    let sizes = |current: f32, options: &'static [f32], mono: bool, cx: &mut Context<Workspace>| {
        let mut row = div().flex().flex_row().gap_1();
        for &size in options {
            row = row.child(
                Button::new(SharedString::from(format!("size-{mono}-{size}")))
                    .xsmall()
                    .when_selected(size == current)
                    .label(format!("{size:.0}"))
                    .on_click(cx.listener(move |_, _, _, cx| {
                        theme::change(cx, |prefs| {
                            if mono {
                                prefs.mono_size = size;
                            } else {
                                prefs.font_size = size;
                            }
                        });
                    })),
            );
        }
        row
    };
    let query = ws.font_search.read(cx).value().trim().to_lowercase();
    let mut fonts = div()
        .id("fonts")
        .max_h(px(220.))
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .rounded_md()
        .border_1()
        .border_color(cx.theme().border)
        .p_1();
    let current = prefs.font.clone();
    fonts = fonts.child(font_row("System default", None, current.is_none(), cx));
    for family in ws
        .fonts
        .iter()
        .filter(|family| query.is_empty() || family.to_lowercase().contains(&query))
        .take(200)
    {
        let chosen = current.as_deref() == Some(family.as_str());
        fonts = fonts.child(font_row(family, Some(family.clone()), chosen, cx));
    }
    div()
        .flex()
        .flex_col()
        .child(widgets::caption("Theme", cx).px_0())
        .child(themes)
        .child(widgets::caption("Fonts", cx).px_0())
        .child(
            line("Interface size", "Text in every column", cx).child(sizes(
                prefs.font_size,
                theme::FONT_SIZES,
                false,
                cx,
            )),
        )
        .child(
            line("Code size", "Commands, paths and code", cx).child(sizes(
                prefs.mono_size,
                theme::MONO_SIZES,
                true,
                cx,
            )),
        )
        .child(line(
            "Interface font",
            &format!(
                "Now {}",
                prefs
                    .font
                    .clone()
                    .unwrap_or_else(|| "the system font".to_owned())
            ),
            cx,
        ))
        .child(div().pt_2().child(Input::new(&ws.font_search)))
        .child(div().pt_2().child(fonts))
}

fn font_row(
    label: &str,
    family: Option<String>,
    chosen: bool,
    cx: &mut Context<Workspace>,
) -> Stateful<Div> {
    let shown = family.clone();
    // A fixed height: a decorative face would otherwise make its row shorter or taller.
    widgets::row(SharedString::from(format!("font-{label}")), chosen, cx)
        .h(px(32.))
        .child(
            div()
                .flex_1()
                .truncate()
                .when_font(shown)
                .child(label.to_owned()),
        )
        .on_click(cx.listener(move |_, _, _, cx| {
            let family = family.clone();
            theme::change(cx, |prefs| prefs.font = family);
        }))
}

trait WhenFont {
    fn when_font(self, family: Option<String>) -> Self;
}

impl WhenFont for Div {
    fn when_font(self, family: Option<String>) -> Self {
        match family {
            Some(family) => self.font_family(family),
            None => self,
        }
    }
}

trait WhenSelected {
    fn when_selected(self, selected: bool) -> Self;
}

impl WhenSelected for Button {
    fn when_selected(self, selected: bool) -> Self {
        if selected {
            self.primary()
        } else {
            self.ghost()
        }
    }
}

fn hex(color: &str) -> Hsla {
    Hsla::parse_hex(color).unwrap_or_default()
}
