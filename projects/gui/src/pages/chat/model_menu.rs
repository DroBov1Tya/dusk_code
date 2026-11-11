//! The menu over the model bar: the current provider's models with a check on the one
//! in use, then Effort and More models, each opening its submenu to the side.

use gpui_kit::{
    component::{ActiveTheme, IconName, Sizable, button::Button, input::Input},
    prelude::FluentBuilder,
    *,
};

use super::model_bar::{picker_heading, picker_hint, picker_panel};
use crate::{
    ui::{Type, widgets},
    workspace::{Center, Picker, Workspace},
};

/// How many models of the current provider the menu lists before More models.
const FIRST: usize = 6;
const WIDTH: f32 = 300.;
const EFFORTS: &[&str] = &[
    "default", "off", "minimal", "low", "medium", "high", "xhigh", "max",
];

pub fn render(
    ws: &Workspace,
    picker: Picker,
    effort: &str,
    viewport: Size<Pixels>,
    cx: &mut Context<Workspace>,
) -> Stateful<Div> {
    let current = ws.status.model.clone().unwrap_or_default();
    let provider = current.split('/').next().unwrap_or_default();
    let max_height = (viewport.height.as_f32() - 165.).clamp(150., 440.);
    let mut main = picker_panel(cx)
        .id("primary-model-list")
        .w(px(WIDTH))
        .max_h(px(max_height))
        .overflow_y_scroll();
    let mine = ws
        .providers
        .iter()
        .find(|p| p.name == provider)
        .or(ws.providers.first());
    let model_heading = mine
        .map(|provider| provider.name.as_str())
        .unwrap_or("Models");
    main = main.child(picker_heading(model_heading, cx));
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
                cx,
            )
            .on_click(cx.listener(|ws, _, _, cx| ws.open_submenu(Picker::MoreModels, cx))),
        )
        .child(separator(cx))
        .child(
            div()
                .px_2()
                .py_1()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .small()
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
                                Input::new(&ws.model_input)
                                    .small()
                                    .aria_label("Custom model (provider/model)"),
                            ),
                        )
                        .child(
                            Button::new("use-custom-model")
                                .small()
                                .outline()
                                .label("Use")
                                .on_click(cx.listener(|ws, _, _, cx| ws.use_typed_model(cx))),
                        ),
                ),
        );
    let side = match picker {
        Picker::Model | Picker::Mode | Picker::Access => None,
        Picker::Effort => Some(efforts(effort, max_height, cx)),
        Picker::MoreModels => Some(all_models(ws, &current, max_height, cx)),
    };
    let side_by_side = viewport.width.as_f32()
        >= if ws.left_hidden { 760. } else { 990. }
            + if ws.right_hidden {
                0.
            } else {
                crate::columns::inspector::WIDTH
            };
    let mut popup = div()
        .id("model-menu")
        .absolute()
        .occlude()
        .bottom(px(36.))
        .right_0()
        .flex()
        .flex_row_reverse()
        .items_end()
        .gap_2();
    popup = match side {
        Some(side) if side_by_side => popup.child(main).child(side),
        Some(side) => popup.child(side),
        None => popup.child(main),
    };
    popup.on_mouse_down_out(cx.listener(|ws, _, _, cx| {
        ws.picker = None;
        ws.picker_closed = Some(std::time::Instant::now());
        cx.notify();
    }))
}

fn catalog_status(ws: &Workspace) -> &'static str {
    if ws.models_loading {
        "Loading the models..."
    } else {
        "No providers are available"
    }
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
    widgets::row(SharedString::from(format!("model-{name}")), chosen, cx)
        .h(px(34.))
        .child(div().flex_1().min_w_0().truncate().child(label.to_owned()))
        .when(chosen, |row| {
            row.child(widgets::icon(IconName::Check, accent))
        })
        .when(!chosen && key.is_some(), |row| {
            row.child(
                div()
                    .meta()
                    .text_color(muted)
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
    cx: &App,
) -> Stateful<Div> {
    let muted = picker_hint(cx);
    widgets::row(id, open, cx)
        .h(px(34.))
        .child(div().flex_1().child(label))
        .children(value.map(|value| div().text_color(muted).child(value)))
        .child(widgets::icon(IconName::ChevronLeft, muted))
}

fn separator(cx: &App) -> Div {
    div().h(px(1.)).mx_2().my_1().bg(cx.theme().border)
}

fn efforts(effort: &str, max_height: f32, cx: &mut Context<Workspace>) -> Div {
    let accent = cx.theme().primary;
    let list = picker_panel(cx).w(px(194.)).child(back_row("Effort", cx));
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
            .py_1p5()
            .child(div().flex_1().child(widgets::capitalize(level)))
            .when(*level == effort, |row| {
                row.child(widgets::icon(IconName::Check, accent))
            })
            .on_click(cx.listener(move |ws, _, _, cx| ws.choose_effort(level, cx))),
        );
    }
    list.child(options)
}

fn all_models(ws: &Workspace, current: &str, max_height: f32, cx: &mut Context<Workspace>) -> Div {
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
                        .flex_1()
                        .min_w_0()
                        .group_name()
                        .truncate()
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
        .w(px(320.))
        .child(back_row("All models", cx))
        .child(scroll)
}

fn back_row(label: &str, cx: &mut Context<Workspace>) -> Stateful<Div> {
    widgets::row("models-back", false, cx)
        .child(widgets::icon(IconName::ChevronLeft, picker_hint(cx)))
        .child(label.to_owned())
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
                if !self.close_top() {
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
