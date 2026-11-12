//! The menu over the model bar: the current provider's models with a check on the one
//! in use, then Effort and More models, each opening its submenu to the side.

use gpui_kit::{
    component::{ActiveTheme, IconName, button::Button, input::Input, tooltip::Tooltip},
    prelude::FluentBuilder,
    *,
};

use super::model_bar::{picker_hint, picker_panel};
use crate::{
    ui::{Type, widgets},
    workspace::{Center, Picker, Workspace},
};

/// How many models of the current provider the menu lists before More models.
const FIRST: usize = 6;
const WIDTH: f32 = 344.;
const ROW: f32 = 36.;
const EFFORTS: &[&str] = &[
    "default", "off", "minimal", "low", "medium", "high", "xhigh", "max",
];

pub fn render(
    ws: &Workspace,
    picker: Picker,
    effort: &str,
    window: &Window,
    cx: &mut Context<Workspace>,
) -> Div {
    let viewport = window.viewport_size();
    let current = ws.status.model.clone().unwrap_or_default();
    let provider = current.split('/').next().unwrap_or_default();
    let max_height = (viewport.height.as_f32() - 165.).clamp(150., 440.);
    let chat_width = ws.chat_width(window);
    let composer_width = chat_width.min(super::composer::WIDTH);
    let rem = window.rem_size().as_f32();
    // Account for the centered composer and the padding, Send and optional
    // Agents button to the right of the Models anchor.
    let agents = if ws.shown_chat().is_some_and(|chat| !chat.agents.is_empty()) {
        super::composer::AGENTS_BUTTON + rem * 0.5
    } else {
        0.
    };
    let reach = (chat_width + composer_width) / 2. - rem * 4.5 - agents - 10.;
    let width = reach.clamp(160., WIDTH);
    let side_by_side = reach >= 2. * WIDTH + rem * 0.5;
    let mut main = picker_panel(cx)
        .id("primary-model-list")
        .w(px(width))
        .max_h(px(max_height))
        .overflow_y_scroll();
    let mine = ws
        .providers
        .iter()
        .find(|p| p.name == provider)
        .or(ws.providers.first());
    let current_in_list = mine.is_some_and(|p| {
        p.models.as_ref().is_ok_and(|models| {
            models
                .iter()
                .take(FIRST)
                .any(|model| current == format!("{}/{}", p.name, model.id))
        })
    });
    main = main.child(models_heading(mine.map(|p| p.name.as_str()), cx));
    if !current.is_empty() && !current_in_list {
        main = main.child(current_model(&current, cx));
    }
    match mine.map(|p| (p, &p.models)) {
        None => main = main.child(widgets::muted(catalog_status(ws), cx)),
        Some((_, Ok(models))) if models.is_empty() => {
            main = main.child(widgets::muted("This provider lists no models", cx));
        }
        Some((p, Err(error))) => {
            main = main.child(widgets::muted(format!("{}: {error}", p.name), cx))
        }
        Some((p, Ok(models))) => {
            for (index, model) in models.iter().take(FIRST).enumerate() {
                let name = format!("{}/{}", p.name, model.id);
                main = main.child(model_row(&name, &model.id, &current, Some(index + 1), cx));
            }
        }
    }
    main = main
        .child(separator(cx))
        .child(
            submenu_row(
                "menu-effort",
                "Effort",
                Some(widgets::capitalize(effort)),
                picker == Picker::Effort,
                side_by_side,
                cx,
            )
            .on_click(cx.listener(|ws, _, _, cx| ws.open_submenu(Picker::Effort, cx))),
        )
        .child(
            submenu_row(
                "menu-more",
                "More models",
                None,
                picker == Picker::MoreModels,
                side_by_side,
                cx,
            )
            .on_click(cx.listener(|ws, _, _, cx| ws.open_submenu(Picker::MoreModels, cx))),
        )
        .child(
            div()
                .p_2()
                .mt_1()
                .rounded_lg()
                .bg(cx.theme().background)
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .group_name()
                        .text_color(picker_hint(cx))
                        .child("Custom model"),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(
                            div().flex_1().min_w_0().child(
                                Styled::h(Input::new(&ws.model_input), px(ROW))
                                    .aria_label("Custom model (provider/model)"),
                            ),
                        )
                        .child(
                            Button::new("use-custom-model")
                                .h(px(36.))
                                .outline()
                                .label("Use")
                                .on_click(cx.listener(|ws, _, _, cx| ws.use_typed_model(cx))),
                        ),
                ),
        );
    let side = match picker {
        Picker::Model | Picker::Mode | Picker::Access => None,
        Picker::Effort => Some(efforts(
            effort,
            max_height,
            width.min(208.),
            side_by_side,
            cx,
        )),
        Picker::MoreModels => Some(all_models(
            ws,
            &current,
            max_height,
            width,
            side_by_side,
            cx,
        )),
    };
    let mut popup = div()
        .id("model-menu")
        .occlude()
        .flex()
        .flex_row_reverse()
        .items_end()
        .gap_2();
    popup = match side {
        Some(side) if side_by_side => popup.child(main).child(side),
        Some(side) => popup.child(side),
        None => popup.child(main),
    };
    div().absolute().bottom(px(36.)).right_0().child(
        anchored()
            .anchor(Anchor::BottomRight)
            .snap_to_window_with_margin(Edges::all(px(8.)))
            .child(popup.on_mouse_down_out(cx.listener(|ws, _, _, cx| {
                ws.picker = None;
                ws.picker_closed = Some(std::time::Instant::now());
                cx.notify();
            }))),
    )
}

fn catalog_status(ws: &Workspace) -> &'static str {
    if ws.models_loading {
        "Loading the models..."
    } else {
        "No providers are available"
    }
}

fn models_heading(provider: Option<&str>, cx: &App) -> Div {
    let muted = picker_hint(cx);
    div()
        .flex()
        .items_center()
        .gap_2()
        .px_3()
        .py_2()
        .child(
            div()
                .body()
                .font_weight(FontWeight::SEMIBOLD)
                .flex_1()
                .child("Models"),
        )
        .children(provider.map(|provider| {
            let label = provider.to_owned();
            div()
                .id("models-provider")
                .max_w(relative(0.62))
                .truncate()
                .rounded_md()
                .px_2()
                .py_1()
                .small()
                .text_color(muted)
                .bg(cx.theme().list_active)
                .tooltip(move |window, cx| Tooltip::new(label.clone()).build(window, cx))
                .child(provider.to_owned())
        }))
}

fn current_model(current: &str, cx: &App) -> Stateful<Div> {
    let label = current.to_owned();
    div()
        .id("models-current")
        .flex()
        .items_center()
        .gap_2()
        .px_3()
        .py_1()
        .small()
        .child(widgets::icon(IconName::Check, cx.theme().primary))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .child(current.to_owned()),
        )
        .child(div().meta().text_color(picker_hint(cx)).child("Selected"))
        .tooltip(move |window, cx| Tooltip::new(label.clone()).build(window, cx))
}

fn model_row(
    name: &str,
    label: &str,
    current: &str,
    key: Option<usize>,
    cx: &mut Context<Workspace>,
) -> Stateful<Div> {
    let muted = picker_hint(cx);
    let accent = cx.theme().primary;
    let chosen = name == current;
    let target = name.to_owned();
    let tooltip = target.clone();
    widgets::row(SharedString::from(format!("model-{name}")), chosen, cx)
        .role(Role::Button)
        .aria_label(target.clone())
        .when(chosen, |row| row.aria_description("Selected model"))
        .h(px(ROW))
        .flex_none()
        .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
        .child(div().flex_1().min_w_0().truncate().child(label.to_owned()))
        .when(chosen, |row| {
            row.child(widgets::icon(IconName::Check, accent))
        })
        .when(!chosen && key.is_some(), |row| {
            row.child(
                div()
                    .meta()
                    .text_color(muted)
                    .px_1p5()
                    .rounded_sm()
                    .bg(cx.theme().background)
                    .child(key.unwrap_or_default().to_string()),
            )
        })
        .on_click(cx.listener(move |ws, _, _, cx| ws.choose_model(target.clone(), cx)))
}

fn submenu_row(
    id: &'static str,
    label: &'static str,
    value: Option<String>,
    open: bool,
    side_by_side: bool,
    cx: &App,
) -> Stateful<Div> {
    let muted = picker_hint(cx);
    widgets::row(id, open, cx)
        .role(Role::Button)
        .aria_expanded(open)
        .h(px(ROW))
        .flex_none()
        .child(div().flex_1().child(label))
        .children(value.map(|value| div().text_color(muted).child(value)))
        .child(widgets::icon(
            if side_by_side {
                IconName::ChevronLeft
            } else {
                IconName::ChevronRight
            },
            muted,
        ))
}

fn separator(cx: &App) -> Div {
    div().h(px(1.)).mx_2().my_1().bg(cx.theme().border)
}

fn efforts(
    effort: &str,
    max_height: f32,
    width: f32,
    side_by_side: bool,
    cx: &mut Context<Workspace>,
) -> Div {
    let accent = cx.theme().primary;
    let list = picker_panel(cx)
        .w(px(width))
        .child(back_row("Effort", side_by_side, cx));
    let mut options = div()
        .id("effort-options")
        .flex()
        .flex_col()
        .max_h(px(max_height - 54.))
        .overflow_y_scroll();
    for level in EFFORTS {
        options = options.child(
            widgets::row(
                SharedString::from(format!("effort-{level}")),
                *level == effort,
                cx,
            )
            .role(Role::Button)
            .h(px(ROW))
            .flex_none()
            .when(*level == effort, |row| {
                row.aria_description("Selected effort")
            })
            .child(div().flex_1().child(widgets::capitalize(level)))
            .when(*level == effort, |row| {
                row.child(widgets::icon(IconName::Check, accent))
            })
            .on_click(cx.listener(move |ws, _, _, cx| ws.choose_effort(level, cx))),
        );
    }
    list.child(options)
}

fn all_models(
    ws: &Workspace,
    current: &str,
    max_height: f32,
    width: f32,
    side_by_side: bool,
    cx: &mut Context<Workspace>,
) -> Div {
    let mut scroll = div()
        .id("all-models")
        .max_h(px(max_height - 54.))
        .flex()
        .flex_col()
        .overflow_y_scroll();
    if ws.providers.is_empty() {
        scroll = scroll.child(widgets::muted(catalog_status(ws), cx));
    }
    for provider in &ws.providers {
        let name = provider.name.clone();
        scroll = scroll.child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .px_3()
                .pt_3()
                .pb_1()
                .child(
                    div()
                        .id(SharedString::from(format!(
                            "models-provider-{}",
                            provider.name
                        )))
                        .flex_1()
                        .min_w_0()
                        .group_name()
                        .truncate()
                        .tooltip(move |window, cx| Tooltip::new(name.clone()).build(window, cx))
                        .child(provider.name.clone()),
                )
                .children(
                    provider
                        .balance
                        .as_ref()
                        .and_then(|balance| balance.as_ref().ok())
                        .map(|balance| {
                            div()
                                .small()
                                .text_color(picker_hint(cx))
                                .child(balance.text())
                        }),
                ),
        );
        match &provider.models {
            Ok(models) if models.is_empty() => {
                scroll = scroll.child(widgets::muted("No listed models", cx));
            }
            Ok(models) => {
                for model in models {
                    let name = format!("{}/{}", provider.name, model.id);
                    scroll = scroll.child(model_row(&name, &model.id, current, None, cx));
                }
            }
            Err(error) => scroll = scroll.child(widgets::muted(error.clone(), cx)),
        }
    }
    picker_panel(cx)
        .w(px(width))
        .child(back_row("All models", side_by_side, cx))
        .child(scroll)
}

fn back_row(label: &str, side_by_side: bool, cx: &mut Context<Workspace>) -> Stateful<Div> {
    widgets::row("models-back", false, cx)
        .role(Role::Button)
        .aria_label("Back to Models")
        .child(widgets::icon(
            if side_by_side {
                IconName::ChevronRight
            } else {
                IconName::ChevronLeft
            },
            picker_hint(cx),
        ))
        .child(
            div()
                .font_weight(FontWeight::SEMIBOLD)
                .child(label.to_owned()),
        )
        .on_click(cx.listener(|ws, _, _, cx| {
            ws.picker = Some(Picker::Model);
            cx.notify();
        }))
}

impl Workspace {
    /// The first models the menu lists, as `provider/model`, each picked by its digit.
    fn first_models(&self) -> Vec<String> {
        let current = self.status.model.clone().unwrap_or_default();
        let provider = current.split('/').next().unwrap_or_default();
        let mine = self
            .providers
            .iter()
            .find(|p| p.name == provider)
            .or(self.providers.first());
        let Some((p, Ok(models))) = mine.map(|p| (p, &p.models)) else {
            return Vec::new();
        };
        models
            .iter()
            .take(FIRST)
            .map(|model| format!("{}/{}", p.name, model.id))
            .collect()
    }

    /// While a menu over the model bar is open, Escape closes it; in the model menu
    /// the digits 1-6 pick its models, before the text box takes them.
    pub fn menu_keys(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let Some(picker) = self.picker else {
            self.page_keys(event, window, cx);
            return;
        };
        let keys = &event.keystroke;
        if keys.modifiers.modified() {
            return;
        }
        if keys.key == "escape" {
            cx.stop_propagation();
            self.picker = None;
            cx.notify();
            return;
        }
        let digit = keys
            .key
            .parse::<usize>()
            .ok()
            .filter(|d| (1..=FIRST).contains(d));
        if picker != Picker::Model || self.model_input.focus_handle(cx).is_focused(window) {
            return;
        }
        if let Some(name) = digit.and_then(|digit| self.first_models().get(digit - 1).cloned()) {
            cx.stop_propagation();
            self.choose_model(name, cx);
        }
    }

    /// Keys of the page in the middle: on the review, Up and Down choose the file and
    /// Escape goes back to the chat. Letters are left alone, so typing still reaches
    /// the boxes.
    fn page_keys(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let keys = &event.keystroke;
        if keys.modifiers.modified() {
            return;
        }
        match keys.key.as_str() {
            "escape" => {
                // Nothing open: Escape leaves the box for the window, from where Tab walks
                // the rows.
                if !self.close_top(window, cx) {
                    if self.focus.is_focused(window) {
                        return;
                    }
                    window.focus(&self.focus, cx);
                }
            }
            "down" | "up" if self.center == Center::Review && self.modal.is_none() => {
                self.review.step(if keys.key == "down" { 1 } else { -1 })
            }
            _ => return,
        }
        cx.stop_propagation();
        cx.notify();
    }

    /// Opens a submenu, or goes back to the menu alone when it is open.
    fn open_submenu(&mut self, submenu: Picker, cx: &mut Context<Self>) {
        self.picker = Some(if self.picker == Some(submenu) {
            Picker::Model
        } else {
            submenu
        });
        cx.notify();
    }
}
