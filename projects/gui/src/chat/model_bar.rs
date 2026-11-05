//! Under the composer: the model and the effort, each with its picker, and how full
//! the context is.

use dusk_protocol::{
    command::{Answer, Command},
    model::Effort,
};
use gpui_kit::{
    component::{
        ActiveTheme, IconName, Sizable,
        button::{Button, ButtonVariants},
    },
    *,
};

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
    let model = ws.status.model.clone().unwrap_or_else(|| {
        if ws.status.discovering_model {
            "looking for a model...".to_owned()
        } else {
            "no model".to_owned()
        }
    });
    let effort = ws.status.effort.map_or("default".to_owned(), effort_name);
    let (used, limit) = ws
        .shown_chat()
        .and_then(|chat| chat.tokens)
        .unwrap_or((0, ws.status.input_limit));
    let share = if limit == 0 { 0. } else { (used as f32 / limit as f32).min(1.) };
    let meter = if share > 0.9 {
        danger
    } else if share > 0.7 {
        warning
    } else {
        accent
    };
    let bar = div()
        .px_6()
        .py_2()
        .max_w(px(900.))
        .mx_auto()
        .w_full()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .text_xs()
        .text_color(muted)
        .child(
            Button::new("pick-model")
                .ghost()
                .xsmall()
                .icon(IconName::Cpu)
                .label(model)
                .on_click(cx.listener(|ws, _, _, cx| ws.toggle_picker(Picker::Model, cx))),
        )
        .child(
            Button::new("pick-effort")
                .ghost()
                .xsmall()
                .icon(IconName::Bot)
                .label(format!("effort: {effort}"))
                .on_click(cx.listener(|ws, _, _, cx| ws.toggle_picker(Picker::Effort, cx))),
        )
        .child(div().flex_1())
        .child(format!("{} / {}", short(used), short(limit)))
        .child(
            div()
                .w(px(96.))
                .h(px(6.))
                .rounded_full()
                .bg(border)
                .child(div().h_full().rounded_full().bg(meter).w(relative(share))),
        );
    let menu = ws.picker.map(|picker| super::model_menu::render(ws, picker, &effort, cx));
    div().relative().child(bar).children(menu)
}

pub fn effort_name(effort: Effort) -> String {
    format!("{effort:?}").to_lowercase()
}

/// `12.3k` for a token count.
fn short(tokens: u64) -> String {
    match tokens {
        0..1_000 => tokens.to_string(),
        1_000..1_000_000 => format!("{:.1}k", tokens as f64 / 1e3),
        _ => format!("{:.1}M", tokens as f64 / 1e6),
    }
}

impl Workspace {
    pub fn toggle_picker(&mut self, picker: Picker, cx: &mut Context<Self>) {
        if self.picker == Some(picker) {
            self.picker = None;
        } else {
            if self.providers.is_empty() {
                self.load_models(cx);
            }
            self.picker = Some(picker);
        }
        cx.notify();
    }

    fn load_models(&mut self, cx: &mut Context<Self>) {
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
