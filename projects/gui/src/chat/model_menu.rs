//! The menu over the model bar: the current provider's models with a check on the one
//! in use, then Effort and More models, each opening its submenu to the side.

use gpui_kit::{
    component::{ActiveTheme, IconName},
    prelude::FluentBuilder,
    *,
};

use crate::{
    widgets,
    workspace::{Picker, Workspace},
};

/// How many models of the current provider the menu lists before More models.
const FIRST: usize = 6;
const WIDTH: f32 = 300.;
const EFFORTS: &[&str] = &["default", "off", "minimal", "low", "medium", "high", "xhigh", "max"];

pub fn render(ws: &Workspace, picker: Picker, effort: &str, cx: &mut Context<Workspace>) -> Div {
    let current = ws.status.model.clone().unwrap_or_default();
    let provider = current.split('/').next().unwrap_or_default();
    let mut main = panel(cx);
    let mine = ws.providers.iter().find(|p| p.name == provider).or(ws.providers.first());
    match mine.map(|p| (p, &p.models)) {
        None => main = main.child(widgets::muted("Loading the models...", cx)),
        Some((p, Err(error))) => main = main.child(widgets::muted(format!("{}: {error}", p.name), cx)),
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
            submenu_row("menu-effort", "Effort", Some(capitalize(effort)), picker == Picker::Effort, cx)
                .on_click(cx.listener(|ws, _, _, cx| ws.open_submenu(Picker::Effort, cx))),
        )
        .child(
            submenu_row("menu-more", "More models", None, picker == Picker::MoreModels, cx)
                .on_click(cx.listener(|ws, _, _, cx| ws.open_submenu(Picker::MoreModels, cx))),
        );
    let side = match picker {
        Picker::Model => None,
        Picker::Effort => Some(efforts(effort, cx)),
        Picker::MoreModels => Some(all_models(ws, &current, cx)),
    };
    div()
        .absolute()
        .bottom(px(40.))
        .left(px(24.))
        .flex()
        .flex_row()
        .items_end()
        .gap_1()
        .child(main)
        .children(side)
}

fn panel(cx: &App) -> Div {
    let theme = cx.theme();
    div()
        .w(px(WIDTH))
        .p_1()
        .flex()
        .flex_col()
        .rounded_xl()
        .border_1()
        .border_color(theme.border)
        .bg(theme.popover)
        .shadow_lg()
        .text_sm()
}

fn model_row(name: &str, label: &str, current: &str, key: Option<usize>, cx: &mut Context<Workspace>) -> Stateful<Div> {
    let muted = cx.theme().muted_foreground;
    let accent = cx.theme().primary;
    let chosen = name == current;
    let target = name.to_owned();
    widgets::row(SharedString::from(format!("model-{name}")), false, cx)
        .py_1p5()
        .child(div().flex_1().min_w_0().truncate().child(label.to_owned()))
        .when(chosen, |row| row.child(widgets::icon(IconName::Check, accent)))
        .when(!chosen && key.is_some(), |row| {
            row.child(div().text_xs().text_color(muted).child(key.unwrap_or_default().to_string()))
        })
        .on_click(cx.listener(move |ws, _, _, cx| ws.choose_model(target.clone(), cx)))
}

fn submenu_row(id: &'static str, label: &'static str, value: Option<String>, open: bool, cx: &App) -> Stateful<Div> {
    let muted = cx.theme().muted_foreground;
    widgets::row(id, open, cx)
        .py_1p5()
        .child(div().flex_1().child(label))
        .children(value.map(|value| div().text_color(muted).child(value)))
        .child(widgets::icon(IconName::ChevronRight, muted))
}

fn separator(cx: &App) -> Div {
    div().h(px(1.)).mx_2().my_1().bg(cx.theme().border)
}

fn efforts(effort: &str, cx: &mut Context<Workspace>) -> Div {
    let accent = cx.theme().primary;
    let mut list = panel(cx).w(px(180.));
    for level in EFFORTS {
        list = list.child(
            widgets::row(SharedString::from(format!("effort-{level}")), false, cx)
                .py_1p5()
                .child(div().flex_1().child(capitalize(level)))
                .when(*level == effort, |row| row.child(widgets::icon(IconName::Check, accent)))
                .on_click(cx.listener(move |ws, _, _, cx| ws.choose_effort(level, cx))),
        );
    }
    list
}

fn all_models(ws: &Workspace, current: &str, cx: &mut Context<Workspace>) -> Div {
    let mut scroll = div().id("all-models").max_h(px(420.)).flex().flex_col().overflow_y_scroll();
    for provider in &ws.providers {
        scroll = scroll.child(widgets::heading(provider.name.clone(), cx));
        match &provider.models {
            Ok(models) => {
                for model in models {
                    let name = format!("{}/{}", provider.name, model.id);
                    scroll = scroll.child(model_row(&name, &model.id, current, None, cx));
                }
            }
            Err(error) => scroll = scroll.child(widgets::muted(error.clone(), cx)),
        }
    }
    panel(cx).w(px(320.)).child(scroll)
}

fn capitalize(text: &str) -> String {
    let mut chars = text.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

impl Workspace {
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
