//! In the composer's bottom row: the model and the effort, the mode and the access, each
//! with its picker opening upward from its own chip, and how full the context is.

use crate::ui::Type;
use dusk_protocol::{
    client::{Answer, Command},
    model::Effort,
};
use gpui_kit::{
    component::{
        ActiveTheme, IconName, Sizable,
        button::{Button, ButtonVariants},
    },
    prelude::FluentBuilder,
    *,
};

use crate::widgets;
use crate::workspace::{Picker, Workspace};

pub fn render(ws: &Workspace, cx: &mut Context<Workspace>) -> Div {
    let theme = cx.theme();
    let (muted, accent, warning, danger, border) = (
        theme.muted_foreground,
        theme.primary,
        theme.warning,
        theme.danger,
        theme.border,
    );
    let (used, limit) = ws
        .shown_chat()
        .and_then(|chat| chat.tokens)
        .unwrap_or((0, ws.status.input_limit));
    let share = if limit == 0 {
        0.
    } else {
        (used as f32 / limit as f32).min(1.)
    };
    let meter = if share > 0.9 {
        danger
    } else if share > 0.7 {
        warning
    } else {
        accent
    };
    let (mode_menu, access_menu) = match ws.picker {
        Some(Picker::Mode) => (Some(popup("mode", super::mode_menu::render(ws, cx))), None),
        Some(Picker::Access) => (
            None,
            Some(popup("access", super::access_menu::render(ws, cx))),
        ),
        _ => (None, None),
    };
    div()
        .flex_1()
        .min_w_0()
        .flex()
        .flex_row()
        .items_center()
        .gap_1()
        .meta()
        .text_color(muted)
        .child(
            div()
                .relative()
                .child(
                    Button::new("mode")
                        .ghost()
                        .xsmall()
                        .label(format!("{} mode", ws.status.mode.to_lowercase()))
                        .tooltip("Pick a mode; Tab walks them")
                        .on_click(cx.listener(|ws, _, _, cx| {
                            if ws.settings.is_none() {
                                ws.load_settings(cx);
                            }
                            ws.toggle_picker(Picker::Mode, cx);
                        })),
                )
                .children(mode_menu),
        )
        .when(ws.shown.is_some(), |bar| {
            bar.child(
                div()
                    .relative()
                    .child(
                        Button::new("access")
                            .ghost()
                            .xsmall()
                            .label(super::access_menu::current(ws).name())
                            .tooltip("What this chat's tools may do without asking")
                            .on_click(
                                cx.listener(|ws, _, _, cx| ws.toggle_picker(Picker::Access, cx)),
                            ),
                    )
                    .children(access_menu),
            )
        })
        .child(div().flex_1())
        .children(ws.shown_chat().and_then(|chat| chat.cost.clone()))
        .child(format!(
            "{} / {}",
            widgets::short(used),
            widgets::short(limit)
        ))
        .child(super::ring::render(share, meter, border))
}

/// The Models button at the right of the composer's row: one click opens the models and
/// the effort, upward from the button.
pub fn models(ws: &Workspace, cx: &mut Context<Workspace>) -> Div {
    let model = ws.status.model.clone().unwrap_or_else(|| {
        if ws.status.discovering_model {
            "looking for a model...".to_owned()
        } else {
            "no model".to_owned()
        }
    });
    let effort = ws.status.effort.map_or("default".to_owned(), effort_name);
    let menu = ws
        .picker
        .filter(|picker| !matches!(picker, Picker::Mode | Picker::Access));
    let menu =
        menu.map(|picker| popup("model", super::model_menu::render(ws, picker, &effort, cx)));
    div()
        .relative()
        .child(
            Button::new("pick-model")
                .ghost()
                .xsmall()
                .icon(IconName::Cpu)
                .label("Models")
                .tooltip(format!("{} \u{00B7} {effort}", short_model(&model)))
                .on_click(cx.listener(|ws, _, _, cx| ws.toggle_picker(Picker::Model, cx))),
        )
        .children(menu)
}

/// A picker's menu, faded in and painted over everything.
fn popup<E>(key: &str, menu: E) -> Deferred
where
    E: IntoElement + Styled + 'static,
{
    widgets::on_top(widgets::appear(format!("composer-menu-{key}"), menu))
}

pub fn effort_name(effort: Effort) -> String {
    format!("{effort:?}").to_lowercase()
}

impl Workspace {
    pub fn toggle_picker(&mut self, picker: Picker, cx: &mut Context<Self>) {
        // The click that closed the menu from outside must not open it again.
        let just_closed = self
            .picker_closed
            .take()
            .is_some_and(|at| at.elapsed() < std::time::Duration::from_millis(300));
        if self.picker == Some(picker) || just_closed {
            self.picker = None;
        } else {
            if self.providers.is_empty() {
                self.load_models(cx);
            }
            self.picker = Some(picker);
        }
        cx.notify();
    }

    pub fn load_models(&mut self, cx: &mut Context<Self>) {
        self.core.call(
            cx,
            |client| async move { client.command(Command::Model { name: None }).await },
            |ws, result, cx| match result {
                Ok(Answer::Providers { providers, .. }) => {
                    ws.providers = providers;
                    cx.notify();
                }
                Ok(_) => ws.fail(anyhow::anyhow!("the core gave no models"), cx),
                Err(error) => ws.fail(error, cx),
            },
        );
    }

    pub fn choose_model(&mut self, name: String, cx: &mut Context<Self>) {
        self.picker = None;
        self.core.call(
            cx,
            move |client| async move { client.command(Command::Model { name: Some(name) }).await },
            |ws, result, cx| {
                if let Err(error) = result {
                    ws.fail(error, cx);
                }
            },
        );
        cx.notify();
    }

    pub fn choose_effort(&mut self, level: &str, cx: &mut Context<Self>) {
        self.picker = None;
        let level = level.to_owned();
        self.core.call(
            cx,
            move |client| async move {
                client.command(Command::Effort { level: Some(level) }).await
            },
            |ws, result, cx| {
                if let Err(error) = result {
                    ws.fail(error, cx);
                }
            },
        );
        cx.notify();
    }
}

/// The model without its provider prefix, cut to a length the bar holds.
fn short_model(name: &str) -> String {
    let name = name.rsplit('/').next().unwrap_or(name);
    if name.chars().count() <= 30 {
        return name.to_owned();
    }
    let head: String = name.chars().take(29).collect();
    format!("{head}\u{2026}")
}
