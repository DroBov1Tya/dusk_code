//! The controls around the message box: the access inside it, the mode and the model
//! under it. Each picker opens upward from its own control.

use crate::ui::Type;
use dusk_protocol::{
    client::{Answer, CHATGPT_USAGE_URL, Command},
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

/// The ChatGPT plan's reported windows, apart from the model's context meter.
pub fn quota(ws: &Workspace, cx: &mut Context<Workspace>) -> Option<Div> {
    if !ws.status.chatgpt_model {
        return None;
    }
    let windows = ws
        .status
        .limits
        .as_ref()
        .map_or(&[][..], |limits| limits.plan.as_slice());
    let usage = Button::new("chatgpt-plan-usage")
        .small()
        .ghost()
        .label("Using ChatGPT plan")
        .icon(IconName::ExternalLink)
        .tooltip("Open ChatGPT usage settings")
        .on_click(cx.listener(|_, _, _, cx| cx.open_url(CHATGPT_USAGE_URL)));
    let row = div()
        .pb_1()
        .flex()
        .items_center()
        .flex_wrap()
        .gap_x_2()
        .child(usage);
    let muted = cx.theme().muted_foreground;
    if windows.is_empty() {
        return Some(row.child(div().meta().text_color(muted).child("Usage unavailable")));
    }
    Some(
        row.children(windows.iter().enumerate().map(|(index, window)| {
            let label = plan_window(window.minutes);
            let text = SharedString::from(format!("{label} {}% used", window.used_percent));
            let tip = match window.resets_in {
                Some(seconds) => {
                    let ms = i64::try_from(seconds).map_or(i64::MAX, |s| s.saturating_mul(1000));
                    SharedString::from(format!(
                        "{text}, resets in {}",
                        dusk_protocol::thread::duration_words(ms)
                    ))
                }
                None => SharedString::from(format!("{text}, last reported by ChatGPT")),
            };
            div()
                .id(("chatgpt-plan-window", index))
                .flex_none()
                .meta()
                .text_color(muted)
                .child(text)
                .tooltip(move |window, cx| {
                    gpui_kit::component::tooltip::Tooltip::new(tip.clone()).build(window, cx)
                })
        })),
    )
}

fn plan_window(minutes: Option<u64>) -> String {
    match minutes {
        Some(10_080) => "Weekly".into(),
        Some(minutes) if minutes.is_multiple_of(1440) => format!("{}d", minutes / 1440),
        Some(minutes) if minutes.is_multiple_of(60) => format!("{}h", minutes / 60),
        Some(minutes) => format!("{minutes}m"),
        None => "Unknown period".into(),
    }
}

/// The Models button at the right of the composer's row: one click opens the models and
/// the effort, upward from the button.
pub fn models(ws: &Workspace, window: &Window, cx: &mut Context<Workspace>) -> Div {
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
        .border_1()
        .border_color(cx.theme().border)
        .bg(cx.theme().popover)
        .shadow_lg()
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
        cx.notify();
        self.core.call(
            cx,
            |client| async move { client.command(Command::Provider { name: None }).await },
            |ws, result, cx| match result {
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
                })
            })
            .collect();
        self.models_pending = names.iter().cloned().collect();
        self.models_loading = !names.is_empty();
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
    if name.chars().count() <= 22 {
        return name.to_owned();
    }
    let head: String = name.chars().take(21).collect();
    format!("{head}\u{2026}")
}
