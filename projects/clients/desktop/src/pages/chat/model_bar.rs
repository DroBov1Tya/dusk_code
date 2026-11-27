//! The controls around the message box: the access inside it, the mode and the model
//! under it. Each picker opens upward from its own control.

use crate::ui::Type;
use crate::ui::elevation::{Elevation, Level};
use dusk_protocol::{
    client::{Answer, Command},
    model::{Effort, ProviderStatus},
};
use gpui_kit::{
    component::{
        ActiveTheme, Colorize, IconName, Sizable,
        button::{Button, ButtonVariants},
    },
    *,
};

use crate::ui::widgets;
use crate::workspace::{Picker, Workspace};

/// The access of the chat's tools, beside the attach button inside the box.
pub fn access(ws: &Workspace, cx: &mut Context<Workspace>) -> Option<Div> {
    ws.shown.as_ref()?;
    let menu = matches!(ws.picker, Some(Picker::Access))
        .then(|| popup("access", super::access_menu::render(ws, cx)));
    Some(
        div()
            .relative()
            .child(
                Button::new("access")
                    .ghost()
                    .icon(IconName::CircleCheck)
                    .label(super::access_menu::current(ws).name())
                    .dropdown_caret(true)
                    .tooltip("What this chat's tools may do without asking")
                    .on_click(cx.listener(|ws, _, _, cx| ws.toggle_picker(Picker::Access, cx))),
            )
            .children(menu),
    )
}

/// The mode, under the box on the left: quiet text with a caret.
pub fn mode(ws: &Workspace, cx: &mut Context<Workspace>) -> Div {
    let menu = matches!(ws.picker, Some(Picker::Mode))
        .then(|| popup("mode", super::mode_menu::render(ws, false, cx)));
    div()
        .relative()
        .child(
            Button::new("mode")
                .ghost()
                .small()
                .label(format!("{} mode", ws.status.mode.to_lowercase()))
                .dropdown_caret(true)
                .tooltip("Pick a mode; Tab walks them")
                .on_click(cx.listener(|ws, _, _, cx| {
                    if ws.settings.is_none() {
                        ws.load_settings(cx);
                    }
                    ws.toggle_picker(Picker::Mode, cx);
                })),
        )
        .children(menu)
}

/// The Models button at the right of the composer's row: one click opens the models and
/// the effort, upward from the button.
pub fn models(ws: &Workspace, window: &Window, cx: &mut Context<Workspace>) -> Div {
    let model = ws.chat_model().unwrap_or_else(|| {
        if ws.status.discovering_model {
            "looking for a model...".to_owned()
        } else {
            "no model".to_owned()
        }
    });
    let effort = ws.chat_effort().map_or("default".to_owned(), effort_name);
    let menu = ws
        .picker
        .filter(|picker| !matches!(picker, Picker::Mode | Picker::Access));
    let menu = menu.map(|picker| {
        popup(
            "model",
            super::model_menu::render(ws, picker, &effort, window, cx),
        )
    });
    div()
        .relative()
        .child(
            Button::new("pick-model")
                .ghost()
                .small()
                .icon(IconName::Cpu)
                .label(short_model(&model))
                .dropdown_caret(true)
                .tooltip(format!("{} \u{00B7} {effort}", short_model(&model)))
                .on_click(cx.listener(|ws, _, _, cx| ws.toggle_picker(Picker::Model, cx))),
        )
        .children(menu)
}

pub(super) fn picker_panel(cx: &App) -> Div {
    div()
        .p_2()
        .flex()
        .flex_col()
        .gap_0p5()
        .rounded_xl()
        .elevation(Level::Floating, cx)
        .body()
}

pub(super) fn picker_hint(cx: &App) -> Hsla {
    let theme = cx.theme();
    theme.foreground.mix_oklab(theme.muted_foreground, 0.16)
}

pub(super) fn picker_heading(label: &str, cx: &App) -> Div {
    div()
        .px_3()
        .py_2()
        .group_name()
        .text_color(picker_hint(cx))
        .child(label.to_owned())
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
    /// The model the shown chat answers on, its own; with no chat shown, or before the
    /// core told it, the one new chats start with.
    pub fn chat_model(&self) -> Option<String> {
        self.shown_chat()
            .and_then(|chat| chat.log.model.clone())
            .or_else(|| self.status.model.clone())
    }

    /// The shown chat's own effort, told with its model; else the one new chats take.
    pub fn chat_effort(&self) -> Option<Effort> {
        match self.shown_chat() {
            Some(chat) if chat.log.model.is_some() => chat.log.effort,
            _ => self.status.effort,
        }
    }

    pub fn toggle_picker(&mut self, picker: Picker, cx: &mut Context<Self>) {
        // The click that closed the menu from outside must not open it again.
        let just_closed = self
            .picker_closed
            .take()
            .is_some_and(|at| at.elapsed() < std::time::Duration::from_millis(300));
        if self.picker == Some(picker) || just_closed {
            self.picker = None;
        } else {
            if matches!(picker, Picker::Model) && self.providers.is_empty() {
                self.load_models(cx);
            }
            self.picker = Some(picker);
        }
        cx.notify();
    }

    /// Lists the providers at once, then asks each for its models on its own, so one that
    /// answers shows while a slow one is still pending.
    pub fn load_models(&mut self, cx: &mut Context<Self>) {
        if self.models_loading {
            return;
        }
        self.models_loading = true;
        self.models_round += 1;
        let round = self.models_round;
        cx.notify();
        self.core.call(
            cx,
            |client| async move { client.command(Command::Provider { name: None }).await },
            move |ws, result, cx| match result {
                _ if ws.models_round != round => {}
                Ok(Answer::Providers { providers, .. }) => {
                    ws.ask_providers(providers.into_iter().map(|p| p.name).collect(), cx)
                }
                other => {
                    ws.models_loading = false;
                    match other {
                        Ok(_) => ws.fail(anyhow::anyhow!("the core gave no providers"), cx),
                        Err(error) => ws.fail(error, cx),
                    }
                    cx.notify();
                }
            },
        );
    }

    /// Loads the models even while a load runs, for after the providers changed: the
    /// running load would show the list from before the change.
    pub fn reload_models(&mut self, cx: &mut Context<Self>) {
        self.models_loading = false;
        self.models_pending.clear();
        self.load_models(cx);
    }

    fn ask_providers(&mut self, names: Vec<String>, cx: &mut Context<Self>) {
        // What an earlier check found stays on screen until the new answer replaces it.
        let mut known: std::collections::HashMap<_, _> = std::mem::take(&mut self.providers)
            .into_iter()
            .map(|status| (status.name.clone(), status))
            .collect();
        self.providers = names
            .iter()
            .map(|name| {
                known.remove(name).unwrap_or_else(|| ProviderStatus {
                    name: name.clone(),
                    models: Ok(Vec::new()),
                    balance: None,
                    limits: None,
                    enabled: true,
                    failure: None,
                })
            })
            .collect();
        self.models_pending = names.iter().cloned().collect();
        self.models_loading = !names.is_empty();
        let round = self.models_round;
        for name in names {
            let asked = name.clone();
            self.core.call(
                cx,
                move |client| async move {
                    client
                        .command(Command::Provider { name: Some(asked) })
                        .await
                },
                move |ws, result, cx| {
                    if ws.models_round != round {
                        return;
                    }
                    ws.models_pending.remove(&name);
                    ws.models_loading = !ws.models_pending.is_empty();
                    let status = match result {
                        Ok(Answer::Providers { providers, .. }) => {
                            match providers.into_iter().next() {
                                Some(status) => status,
                                None => {
                                    return ws.provider_failed(
                                        &name,
                                        "the core gave no answer",
                                        cx,
                                    );
                                }
                            }
                        }
                        // A provider that fails shows its own error; the others go on.
                        Ok(_) => return ws.provider_failed(&name, "the core gave no answer", cx),
                        Err(error) => return ws.provider_failed(&name, &format!("{error:#}"), cx),
                    };
                    if let Some(slot) = ws.providers.iter_mut().find(|p| p.name == status.name) {
                        *slot = status;
                    }
                    cx.notify();
                },
            );
        }
        cx.notify();
    }

    fn provider_failed(&mut self, name: &str, error: &str, cx: &mut Context<Self>) {
        if let Some(slot) = self.providers.iter_mut().find(|p| p.name == name) {
            slot.models = Err(error.to_owned());
        }
        cx.notify();
    }

    pub fn choose_model(&mut self, name: String, cx: &mut Context<Self>) {
        self.picker = None;
        let thread_id = self.shown.clone();
        self.core.call(
            cx,
            // The shown chat and new chats take it; another chat keeps its own.
            move |client| async move {
                let name = Some(name);
                client.command(Command::Model { name, thread_id }).await
            },
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
        let (level, thread_id) = (Some(level.to_owned()), self.shown.clone());
        self.core.call(
            cx,
            move |client| async move { client.command(Command::Effort { level, thread_id }).await },
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
    if name.chars().count() <= 22 {
        return name.to_owned();
    }
    let head: String = name.chars().take(21).collect();
    format!("{head}\u{2026}")
}
