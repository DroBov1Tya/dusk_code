//! The providers that are connected: what each answered, and what can be done with it.

use dusk_protocol::{client::ProviderTarget, model::ProviderStatus};
use gpui_kit::{
    component::{
        Disableable, Sizable,
        button::{Button, ButtonVariants},
        input::Input,
    },
    *,
};

use super::{card, check::Check};
use crate::{
    ui::{Type, widgets},
    workspace::Workspace,
};

pub fn render(ws: &Workspace, cx: &mut Context<Workspace>) -> Div {
    let current = ws.status.model.clone().unwrap_or_default();
    let mut column = div().flex().flex_col().gap_3().child(
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .child(
                div()
                    .flex_1()
                    .body()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("Providers"),
            )
            .when_some((!ws.providers.is_empty()).then_some(()), |row, _| {
                row.child(
                    Button::new("provider-check-all")
                        .small()
                        .outline()
                        .label("Check all")
                        .tooltip("Ask every provider whether it answers")
                        .on_click(cx.listener(|ws, _, _, cx| ws.check_all_providers(cx))),
                )
                .child(
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
        column = column.child(provider_card(ws, provider, &current, cx));
    }
    column
}

use gpui_kit::prelude::FluentBuilder;

fn provider_card(
    ws: &Workspace,
    provider: &ProviderStatus,
    current: &str,
    cx: &mut Context<Workspace>,
) -> Div {
    let name = provider.name.clone();
    let in_use = current.starts_with(&format!("{name}/"));
    // A check made here replaces what the list was loaded with.
    let check = ws
        .providers_page
        .checks
        .get(&name)
        .cloned()
        .unwrap_or_else(|| Check::of(provider));
    let running = matches!(check, Check::Running);
    let reconnecting = ws.providers_page.reconnecting.as_deref() == Some(name.as_str());
    let (check_name, again_name, remove_name) = (name.clone(), name.clone(), name.clone());
    let buttons = div()
        .flex()
        .flex_row()
        .items_center()
        .gap_1()
        .child(
            Button::new(SharedString::from(format!("provider-check-{name}")))
                .small()
                .outline()
                .label("Check")
                .disabled(running)
                .tooltip("Ask this provider whether it answers")
                .on_click(cx.listener(move |ws, _, _, cx| ws.check_provider(&check_name, cx))),
        )
        .child(
            Button::new(SharedString::from(format!("provider-again-{name}")))
                .small()
                .ghost()
                .label("Reconnect")
                .tooltip("Connect again, with a new key if the old one stopped working")
                .on_click(cx.listener(move |ws, _, _, cx| ws.start_reconnect(&again_name, cx))),
        )
        .child(
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
                .on_click(cx.listener(move |ws, _, _, cx| ws.remove_provider(&remove_name, cx))),
        );
    let mut card = card(cx)
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_3()
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .body()
                        .font_weight(FontWeight::MEDIUM)
                        .truncate()
                        .child(name.clone()),
                )
                .child(buttons),
        )
        .child(check.line("Connected", cx));
    if reconnecting {
        card = card
            .child(Input::new(&ws.providers_page.reconnect_key).large())
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap_2()
                    .child(
                        Button::new("provider-again-go")
                            .primary()
                            .label("Reconnect")
                            .on_click(cx.listener(|ws, _, window, cx| ws.reconnect(window, cx))),
                    )
                    .child(
                        Button::new("provider-again-cancel")
                            .ghost()
                            .label("Cancel")
                            .on_click(cx.listener(|ws, _, _, cx| {
                                ws.providers_page.reconnecting = None;
                                cx.notify();
                            })),
                    ),
            );
    }
    card
}

impl Workspace {
    /// Asks a saved provider whether it answers.
    pub(super) fn check_provider(&mut self, name: &str, cx: &mut Context<Self>) {
        self.providers_page
            .checks
            .insert(name.to_owned(), Check::Running);
        let (asked, shown) = (name.to_owned(), name.to_owned());
        self.core.call(
            cx,
            move |client| async move {
                client
                    .settings()
                    .check_provider(ProviderTarget::Saved { name: asked })
                    .await
            },
            move |ws, result, cx| {
                let found = match result {
                    Ok(status) => Check::of(&status),
                    Err(error) => Check::failed(&error),
                };
                ws.providers_page.checks.insert(shown, found);
                cx.notify();
            },
        );
        cx.notify();
    }

    pub(super) fn check_all_providers(&mut self, cx: &mut Context<Self>) {
        let names: Vec<String> = self.providers.iter().map(|p| p.name.clone()).collect();
        for name in names {
            self.check_provider(&name, cx);
        }
    }

    /// Opens the box for a new key under a provider.
    pub(super) fn start_reconnect(&mut self, name: &str, cx: &mut Context<Self>) {
        self.providers_page.reconnecting = Some(name.to_owned());
        cx.notify();
    }

    /// Connects the provider anew with the key in the box, or the saved one when it is empty,
    /// and shows what it answers.
    pub(super) fn reconnect(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(name) = self.providers_page.reconnecting.take() else {
            return;
        };
        let box_ = self.providers_page.reconnect_key.clone();
        let key = Some(box_.read(cx).value().to_string()).filter(|key| !key.trim().is_empty());
        box_.update(cx, |input, cx| input.set_value("", window, cx));
        self.providers_page
            .checks
            .insert(name.clone(), Check::Running);
        let (asked, shown) = (name.clone(), name);
        self.core.call(
            cx,
            move |client| async move {
                client
                    .settings()
                    .reconnect_provider(&asked, key.as_deref())
                    .await
            },
            move |ws, result, cx| {
                let found = match result {
                    Ok(status) => Check::of(&status),
                    Err(error) => Check::failed(&error),
                };
                ws.providers_page.checks.insert(shown, found);
                ws.load_models(cx);
            },
        );
        cx.notify();
    }

    pub(super) fn remove_provider(&mut self, name: &str, cx: &mut Context<Self>) {
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
