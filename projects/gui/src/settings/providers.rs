//! The providers on the Model page: the ones connected, each with what it answered and a
//! way to forget it, and a form that connects another one. The key is typed masked and
//! goes to the core only.

use dusk_protocol::{config::ProviderKind, providers::ProviderStatus};
use gpui_kit::{
    component::{
        ActiveTheme, Disableable, Sizable,
        button::{Button, ButtonVariants},
        input::{Input, InputState},
    },
    prelude::FluentBuilder,
    *,
};

use super::line;
use crate::{widgets, workspace::Workspace};

const KINDS: [(ProviderKind, &str); 3] = [
    (ProviderKind::OpenAiCompatible, "Custom"),
    (ProviderKind::OpenAi, "OpenAI"),
    (ProviderKind::Anthropic, "Anthropic"),
];

/// The boxes of the form that connects a provider.
pub struct Form {
    kind: ProviderKind,
    name: Entity<InputState>,
    url: Entity<InputState>,
    key: Entity<InputState>,
}

impl Form {
    pub fn new(window: &mut Window, cx: &mut App) -> Self {
        let field = |placeholder: &'static str, masked: bool, window: &mut Window, cx: &mut App| {
            cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(placeholder)
                    .masked(masked)
            })
        };
        Self {
            kind: ProviderKind::default(),
            name: field("Name (optional)", false, window, cx),
            url: field("https://host/v1", false, window, cx),
            key: field("API key (optional for a local server)", true, window, cx),
        }
    }
}

pub fn render(ws: &Workspace, cx: &mut Context<Workspace>) -> Div {
    let muted = cx.theme().muted_foreground;
    let current = ws.status.model.clone().unwrap_or_default();
    let mut column = div().flex().flex_col().pt_4().child(
        div()
            .flex()
            .flex_row()
            .items_center()
            .child(widgets::caption("Providers", cx).px_0().flex_1())
            .when(!ws.providers.is_empty(), |row| {
                row.child(
                    Button::new("provider-remove-all")
                        .small()
                        .ghost()
                        .label("Remove all")
                        .tooltip("Forget every provider and its key")
                        .on_click(cx.listener(|ws, _, _, cx| {
                            ws.modal = Some(crate::modal::Modal::RemoveProviders);
                            cx.notify();
                        })),
                )
            }),
    );
    if ws.providers.is_empty() {
        column = column.child(widgets::muted("Asking the providers...", cx).px_0());
    }
    for provider in &ws.providers {
        column = column.child(provider_line(provider, &current, cx));
    }
    let mut kinds = div().flex().flex_row().gap_1();
    for (kind, label) in KINDS {
        let chosen = ws.provider_form.kind == kind;
        kinds = kinds.child(
            Button::new(("provider-kind", kind as usize))
                .small()
                .when(chosen, |button| button.primary())
                .when(!chosen, |button| button.ghost())
                .label(label)
                .on_click(
                    cx.listener(move |ws, _, window, cx| ws.pick_provider_kind(kind, window, cx)),
                ),
        );
    }
    column
        .child(
            div()
                .pt_6()
                .child(widgets::caption("Connect a provider", cx).px_0()),
        )
        .child(
            div().text_sm().text_color(muted).child(
                "Several can be connected at once; the model menu lists the models of each.",
            ),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .pt_2()
                .max_w(px(520.))
                .child(kinds)
                .child(Input::new(&ws.provider_form.name))
                .child(Input::new(&ws.provider_form.url))
                .child(Input::new(&ws.provider_form.key))
                .child(
                    div().flex().flex_row().child(
                        Button::new("provider-connect")
                            .small()
                            .primary()
                            .label("Connect")
                            .on_click(
                                cx.listener(|ws, _, window, cx| ws.connect_provider(window, cx)),
                            ),
                    ),
                ),
        )
}

/// What the one button forgets, said before it does.
pub fn confirm_remove_all(cx: &mut Context<Workspace>) -> Div {
    let muted = cx.theme().muted_foreground;
    div()
        .flex()
        .flex_col()
        .gap_3()
        .p_5()
        .child("Remove every connected provider?")
        .child(div().text_sm().text_color(muted).child(
            "Their API keys are forgotten and the chosen model is cleared. Chats stay, but \
             cannot be continued until a provider is connected again.",
        ))
        .child(
            div()
                .flex()
                .flex_row()
                .justify_end()
                .gap_2()
                .child(
                    Button::new("providers-cancel")
                        .ghost()
                        .label("Cancel")
                        .on_click(cx.listener(|ws, _, _, cx| ws.close_modal(cx))),
                )
                .child(
                    Button::new("providers-remove-all")
                        .danger()
                        .label("Remove all")
                        .on_click(cx.listener(|ws, _, _, cx| ws.remove_all_providers(cx))),
                ),
        )
}

fn provider_line(provider: &ProviderStatus, current: &str, cx: &mut Context<Workspace>) -> Div {
    let state = match &provider.models {
        Ok(models) => format!("{} models", models.len()),
        Err(error) => error.clone(),
    };
    let hint = match &provider.balance {
        Some(Ok(balance)) => format!("{state} \u{00B7} {}", balance.text()),
        _ => state,
    };
    let name = provider.name.clone();
    let in_use = current.starts_with(&format!("{name}/"));
    line(&provider.name, &hint, cx).child(
        Button::new(SharedString::from(format!("provider-remove-{name}")))
            .small()
            .ghost()
            .label("Remove")
            .disabled(in_use)
            .tooltip(if in_use {
                "It holds the chosen model; choose another first"
            } else {
                "Forget this provider and its key"
            })
            .on_click(cx.listener(move |ws, _, _, cx| ws.remove_provider(&name, cx))),
    )
}

impl Workspace {
    /// Takes the provider's own address into the box, when the kind has one.
    fn pick_provider_kind(
        &mut self,
        kind: ProviderKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.provider_form.kind = kind;
        if let Some(url) = kind.base_url() {
            self.provider_form
                .url
                .update(cx, |url_box, cx| url_box.set_value(url, window, cx));
        }
        cx.notify();
    }

    fn connect_provider(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let form = &self.provider_form;
        let kind = form.kind;
        let name =
            Some(form.name.read(cx).value().trim().to_owned()).filter(|name| !name.is_empty());
        let url = form.url.read(cx).value().trim().to_owned();
        let key = Some(form.key.read(cx).value().to_string()).filter(|key| !key.is_empty());
        for field in [&form.name, &form.url, &form.key] {
            field.update(cx, |field, cx| field.set_value("", window, cx));
        }
        self.core.call(
            cx,
            move |client| async move { client.settings().connect(kind, name, url, key).await },
            |ws, result, cx| match result {
                Ok(_) => ws.load_models(cx),
                Err(error) => ws.fail(error, cx),
            },
        );
    }

    fn remove_all_providers(&mut self, cx: &mut Context<Self>) {
        self.modal = None;
        self.core.call(
            cx,
            |client| async move { client.settings().remove_all_providers().await },
            |ws, result, cx| match result {
                Ok(()) => ws.load_models(cx),
                Err(error) => ws.fail(error, cx),
            },
        );
        cx.notify();
    }

    fn remove_provider(&mut self, name: &str, cx: &mut Context<Self>) {
        let name = name.to_owned();
        self.core.call(
            cx,
            move |client| async move { client.settings().remove_provider(&name).await },
            |ws, result, cx| match result {
                Ok(()) => ws.load_models(cx),
                Err(error) => ws.fail(error, cx),
            },
        );
    }
}
