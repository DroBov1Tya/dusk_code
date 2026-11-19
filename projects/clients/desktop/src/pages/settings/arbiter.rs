//! Auto mode's arbiter on the Safety page: the model it judges on and its review prompt.

use dusk_protocol::config::{Arbiter, MAX_ARBITER_PROMPT_CHARS};
use gpui_kit::{
    component::{
        ActiveTheme,
        button::{Button, ButtonVariants},
        input::{Input, InputState, Textarea, TextareaState},
    },
    *,
};

use super::{line, list};
use crate::{ui::Type, workspace::Workspace};

pub struct ArbiterForm {
    model: Entity<InputState>,
    prompt: Entity<TextareaState>,
    /// What the fields were last filled from, so a reload refills them only on a change.
    shown: Option<Arbiter>,
}

impl ArbiterForm {
    pub fn new(window: &mut Window, cx: &mut App) -> Self {
        Self {
            model: cx.new(|cx| InputState::new(window, cx).placeholder("the chat's model")),
            prompt: cx.new(|cx| TextareaState::new(window, cx).auto_grow(8, 20)),
            shown: None,
        }
    }
}

pub fn render(ws: &mut Workspace, window: &mut Window, cx: &mut Context<Workspace>) -> Div {
    let Some(settings) = ws.settings.as_ref() else {
        return div();
    };
    let (saved, default) = (
        settings.arbiter.clone(),
        settings.arbiter_default_prompt.clone(),
    );
    if ws.arbiter.shown.as_ref() != Some(&saved) {
        let model = saved.model.clone().unwrap_or_default();
        let prompt = saved.prompt.clone().unwrap_or(default);
        ws.arbiter
            .model
            .update(cx, |field, cx| field.set_value(model, window, cx));
        ws.arbiter
            .prompt
            .update(cx, |field, cx| field.set_value(prompt, window, cx));
        ws.arbiter.shown = Some(saved);
    }
    let muted = cx.theme().muted_foreground;
    div()
        .flex()
        .flex_col()
        .gap_3()
        .child(div().font_weight(FontWeight::MEDIUM).child("Auto mode"))
        .child(list(
            [
                line("Model", "Judges the approvals; empty uses the chat's", cx)
                    .child(div().w(px(240.)).child(Input::new(&ws.arbiter.model))),
            ],
            cx,
        ))
        .child(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .child(div().small().child("Review prompt"))
                .child(div().small().text_color(muted).child(format!(
                    "Guides the judgment only; it cannot widen what the approval mode \
                             allows. Unclear answers go to you. Up to \
                             {MAX_ARBITER_PROMPT_CHARS} characters."
                )))
                .child(Textarea::new(&ws.arbiter.prompt))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .gap_2()
                        .child(
                            Button::new("arbiter-save")
                                .primary()
                                .label("Save")
                                .on_click(cx.listener(|ws, _, _, cx| ws.save_arbiter(false, cx))),
                        )
                        .child(
                            Button::new("arbiter-reset")
                                .outline()
                                .label("Reset to default")
                                .on_click(cx.listener(|ws, _, _, cx| ws.save_arbiter(true, cx))),
                        ),
                ),
        )
}

impl Workspace {
    /// The prompt is saved only when it differs from the built-in one, so a later update
    /// of the built-in prompt still reaches a user who never changed it.
    fn save_arbiter(&mut self, reset: bool, cx: &mut Context<Self>) {
        let default = self
            .settings
            .as_ref()
            .map(|settings| settings.arbiter_default_prompt.clone())
            .unwrap_or_default();
        let model = self.arbiter.model.read(cx).value().trim().to_owned();
        let prompt = self.arbiter.prompt.read(cx).value().trim().to_owned();
        let arbiter = Arbiter {
            model: Some(model),
            prompt: (!reset && prompt != default.trim()).then_some(prompt),
        };
        // Refilled from what the core saved.
        self.arbiter.shown = None;
        self.core.call(
            cx,
            move |client| async move { client.settings().arbiter(arbiter).await },
            |ws, result, cx| match result {
                Ok(()) => ws.load_settings(cx),
                Err(error) => ws.fail(error, cx),
            },
        );
    }
}
