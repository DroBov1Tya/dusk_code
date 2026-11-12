//! How the window looks: the theme, and the fonts with their sizes.

use gpui_kit::{
    component::{ActiveTheme, Colorize, IconName, Sizable, button::Button, input::Input},
    prelude::FluentBuilder,
    *,
};

use super::{section, section_hint_color};
use crate::{
    ui::{
        Type, theme,
        widgets::{self, Focusable},
    },
    workspace::Workspace,
};

pub fn render(ws: &mut Workspace, _: &mut Window, cx: &mut Context<Workspace>) -> Div {
    let prefs = theme::prefs(cx).clone();
    let hover = cx.theme().accent;
    let mut themes = div().flex().flex_wrap().gap_3();
    for palette in theme::PALETTES {
        let name = palette.name;
        let chosen = name == prefs.theme;
        themes = themes.child(
            div()
                .id(SharedString::from(format!("theme-{name}")))
                .focusable_row(cx)
                .flex()
                .flex_col()
                // Keep the same column width on a row with fewer themes.
                .w(px(200.))
                .max_w_full()
                .gap_2()
                .p_2()
                .rounded_lg()
                .border_color(if chosen {
                    cx.theme().ring
                } else {
                    cx.theme().border
                })
                .bg(cx.theme().background)
                .cursor_pointer()
                .hover(move |tile| tile.bg(hover))
                .child(
                    div()
                        .h(px(52.))
                        .flex()
                        .overflow_hidden()
                        .rounded_md()
                        .bg(hex(palette.background))
                        .child(div().w(relative(0.24)).bg(hex(palette.panel)))
                        .child(
                            div()
                                .flex_1()
                                .flex()
                                .items_center()
                                .justify_between()
                                .gap_2()
                                .px_3()
                                .text_size(rems(1.3))
                                .text_color(hex(palette.foreground))
                                .child("Aa")
                                .child(
                                    div()
                                        .size_3()
                                        .flex_shrink_0()
                                        .rounded_full()
                                        .bg(hex(palette.accent)),
                                ),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap_1()
                        .px_1()
                        .h_6()
                        .group_name()
                        .child(name)
                        .when(chosen, |row| {
                            row.child(widgets::icon(IconName::Check, cx.theme().primary))
                        }),
                )
                .on_click(cx.listener(move |_, _, _, cx| {
                    theme::change(cx, |prefs| prefs.theme = name.to_owned());
                })),
        );
    }
    let sizes = |current: f32, options: &'static [f32], mono: bool, cx: &mut Context<Workspace>| {
        let mut row = div().flex().flex_wrap().gap_1();
        for &size in options {
            row = row.child(
                crate::ui::controls::choice(
                    Button::new(SharedString::from(format!("size-{mono}-{size}"))).compact(),
                    size == current,
                    cx,
                )
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
        .max_h(px(236.))
        .overflow_y_scroll()
        // The wheel over the list moves the list, not the page under it.
        .occlude()
        .flex()
        .flex_col()
        .rounded_md()
        .bg(cx.theme().background)
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
    let current_label = current.as_deref().unwrap_or("System default");
    let font_controls = div()
        .flex()
        .flex_col()
        .gap_2()
        .child(
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .justify_between()
                .gap_2()
                .child(div().body().child("Interface font"))
                .child(
                    div()
                        .small()
                        .text_color(section_hint_color(cx))
                        .child(current_label.to_owned()),
                ),
        )
        .child(
            Input::new(&ws.font_search)
                .large()
                .aria_label("Find a font"),
        )
        .child(fonts);
    div()
        .flex()
        .flex_col()
        .gap_4()
        .child(section("Theme", "", cx).child(themes))
        .child(
            section("Atmosphere", "", cx)
                .child(type_size(
                    "Sky",
                    "While the agent works, the header glows and fills with stars",
                    div().child(
                        gpui_kit::component::switch::Switch::new("appearance-sky")
                            .checked(prefs.sky)
                            .on_click(cx.listener(|_, on: &bool, _, cx| {
                                let on = *on;
                                theme::change(cx, move |prefs| prefs.sky = on);
                            })),
                    ),
                    cx,
                ))
                .child(type_size(
                    "Mist",
                    "Rises at the foot of the window left alone; a click clears it",
                    div().child(
                        gpui_kit::component::switch::Switch::new("appearance-fog")
                            .checked(prefs.fog)
                            .on_click(cx.listener(|_, on: &bool, _, cx| {
                                let on = *on;
                                theme::change(cx, move |prefs| prefs.fog = on);
                            })),
                    ),
                    cx,
                )),
        )
        .child(
            section("Typography", "", cx)
                .child(type_size(
                    "Interface size",
                    "Text in every column",
                    sizes(prefs.font_size, theme::FONT_SIZES, false, cx),
                    cx,
                ))
                .child(type_size(
                    "Code size",
                    "Commands, paths and code",
                    sizes(prefs.mono_size, theme::MONO_SIZES, true, cx),
                    cx,
                ))
                .child(font_controls),
        )
}

fn type_size(label: &str, hint: &str, control: Div, cx: &App) -> Div {
    div()
        .flex()
        .flex_wrap()
        .items_center()
        .justify_between()
        .gap_2()
        .child(
            div()
                .flex()
                .flex_col()
                .gap_0p5()
                .child(div().body().child(label.to_owned()))
                .child(
                    div()
                        .small()
                        .text_color(section_hint_color(cx))
                        .child(hint.to_owned()),
                ),
        )
        .child(control)
}

fn font_row(
    label: &str,
    family: Option<String>,
    chosen: bool,
    cx: &mut Context<Workspace>,
) -> Stateful<Div> {
    let accent = cx.theme().primary;
    let shown = family.clone();
    // A fixed height: a decorative face would otherwise make its row shorter or taller.
    widgets::row(SharedString::from(format!("font-{label}")), chosen, cx)
        .h(px(36.))
        .child(
            div()
                .flex_1()
                .truncate()
                .when_font(shown)
                .child(label.to_owned()),
        )
        .when(chosen, |row| {
            row.child(widgets::icon(IconName::Check, accent))
        })
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

fn hex(color: &str) -> Hsla {
    Hsla::parse_hex(color).unwrap_or_default()
}
