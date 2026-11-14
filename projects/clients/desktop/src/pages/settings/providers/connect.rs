//! The card that connects a provider: its kind, its address and key, and a check that finds
//! out whether it answers before anything is saved.

use dusk_protocol::{
    client::{ChatGpt, ProviderTarget},
    config::ProviderKind,
};
use gpui_kit::{
    component::{
        ActiveTheme, Disableable, IconName, Sizable,
        button::{Button, ButtonVariants},
        input::Input,
    },
    prelude::FluentBuilder,
    *,
};

use super::check::Check;
use crate::{
    ui::{
        Type,
        widgets::{card, labeled},
    },
    workspace::Workspace,
};

const KINDS: [(ProviderKind, &str, &str); 4] = [
    (
        ProviderKind::OpenAiCompatible,
        "Custom",
        "Any server with the OpenAI API",
    ),
    (ProviderKind::OpenAi, "OpenAI", "api.openai.com"),
    (ProviderKind::Anthropic, "Anthropic", "api.anthropic.com"),
    (
        ProviderKind::Google,
        "Google AI",
        "generativelanguage.googleapis.com",
    ),
];

/// Signing in with a ChatGPT account: no key, the browser asks and the plan pays.
fn chatgpt(ws: &Workspace, cx: &mut Context<Workspace>) -> Div {
    let (muted, danger, border) = {
        let theme = cx.theme();
        (theme.muted_foreground, theme.danger, theme.border)
    };
    let state = &ws.status.chatgpt;
    let (said, color) = match state {
        ChatGpt::SignedOut => (
            "Use the models of your ChatGPT plan, with no API key".to_owned(),
            muted,
        ),
        ChatGpt::Waiting => ("Waiting for the sign-in in your browser".to_owned(), muted),
        ChatGpt::SignedIn { account } => (format!("Signed in as {account}"), muted),
        ChatGpt::Failed(error) => (error.clone(), danger),
    };
    let label = match state {
        ChatGpt::SignedIn { .. } => "Sign in again",
        _ => "Sign in with ChatGPT",
    };
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap_3()
        .p_3()
        .rounded_lg()
        .border_1()
        .border_color(border)
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap_0p5()
                .child(
                    div()
                        .body()
                        .font_weight(FontWeight::MEDIUM)
                        .child("ChatGPT"),
                )
                .child(div().small().text_color(color).child(said)),
        )
        // While the browser is out, the page can be opened in another browser too.
        .when_some(
            ws.providers_page
                .sign_in_url
                .clone()
                .filter(|_| *state == ChatGpt::Waiting),
            |row, url| {
                row.child(
                    Button::new("provider-chatgpt-copy")
                        .ghost()
                        .icon(IconName::Copy)
                        .label("Copy URL")
                        .tooltip("Copy the sign-in page to open it in another browser")
                        .on_click(move |_, _, cx| {
                            cx.write_to_clipboard(ClipboardItem::new_string(url.clone()))
                        }),
                )
            },
        )
        .child(
            Button::new("provider-chatgpt")
                .outline()
                .label(label)
                .disabled(*state == ChatGpt::Waiting)
                .tooltip("Opens OpenAI's sign-in page in your browser")
                .on_click(cx.listener(|ws, _, _, cx| ws.sign_in_chatgpt(cx))),
        )
}

impl Workspace {
    /// The core answers with OpenAI's sign-in page; the system browser opens it, and the
    /// status says how it ended.
    fn sign_in_chatgpt(&mut self, cx: &mut Context<Self>) {
        self.core.call(
            cx,
            |client| async move { client.settings().sign_in_chatgpt().await },
            |ws, result, cx| match result {
                Ok(url) if url.starts_with("https://auth.openai.com/") => {
                    cx.open_url(&url);
                    ws.providers_page.sign_in_url = Some(url);
                    cx.notify();
                }
                Ok(_) => ws.fail(anyhow::anyhow!("refused to open the sign-in page"), cx),
                Err(error) => ws.fail(error, cx),
            },
        );
    }
}

pub fn render(ws: &Workspace, cx: &mut Context<Workspace>) -> Div {
    let page = &ws.providers_page;
    let (muted, accent, border) = {
        let theme = cx.theme();
        (theme.muted_foreground, theme.primary, theme.border)
    };
    let (label, about) = KINDS
        .iter()
        .find(|(kind, ..)| *kind == page.kind)
        .map_or(("Custom", ""), |(_, label, about)| (*label, *about));
    // The kinds as one drop-down: the chosen one on the button, the rest under it.
    let mut kinds = div().flex().flex_col().gap_1().child(
        div()
            .id("provider-kind")
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .h(px(40.))
            .px_3()
            .rounded_lg()
            .border_1()
            .border_color(if page.kinds_open { accent } else { border })
            .cursor_pointer()
            .child(div().body().font_weight(FontWeight::MEDIUM).child(label))
            .child(div().flex_1().small().text_color(muted).child(about))
            .child(crate::ui::widgets::icon(
                if page.kinds_open {
                    IconName::ChevronUp
                } else {
                    IconName::ChevronDown
                },
                muted,
            ))
            .on_click(cx.listener(|ws, _, _, cx| {
                ws.providers_page.kinds_open = !ws.providers_page.kinds_open;
                cx.notify();
            })),
    );
    if page.kinds_open {
        let mut list = div()
            .flex()
            .flex_col()
            .p_1()
            .rounded_lg()
            .border_1()
            .border_color(border)
            .bg(cx.theme().popover);
        for (kind, label, about) in KINDS {
            let chosen = page.kind == kind;
            list = list.child(
                crate::ui::widgets::row(("provider-kind", kind as usize), chosen, cx)
                    .child(div().body().child(label))
                    .child(div().flex_1().small().text_color(muted).child(about))
                    .when(chosen, |row| {
                        row.child(crate::ui::widgets::icon(IconName::Check, accent))
                    })
                    .on_click(cx.listener(move |ws, _, window, cx| {
                        ws.pick_provider_kind(kind, window, cx)
                    })),
            );
        }
        kinds = kinds.child(list);
    }
    let checking = matches!(page.form_check, Some(Check::Running));
    card(cx)
        .child(
            div()
                .body()
                .font_weight(FontWeight::SEMIBOLD)
                .child("Connect a provider"),
        )
        .child(
            div().small().text_color(muted).child(
                "Several can be connected at once; the model menu lists the models of each.",
            ),
        )
        .child(labeled("Provider", "whose API the server speaks", cx).child(kinds))
        // OpenAI can also be reached through a ChatGPT plan, without a key.
        .when(page.kind == ProviderKind::OpenAi, |form| {
            form.child(chatgpt(ws, cx))
        })
        .child(
            labeled("Provider name", "optional; it shows in the model menu", cx)
                .child(Input::new(&page.name).large()),
        )
        // The known providers have one address; only a custom server needs one typed.
        .when(page.kind == ProviderKind::OpenAiCompatible, |form| {
            form.child(
                labeled("Address", "where the server is", cx).child(Input::new(&page.url).large()),
            )
        })
        .child(
            labeled(
                "API key",
                if page.kind == ProviderKind::OpenAi {
                    "or a key from platform.openai.com; kept in the system keychain"
                } else {
                    "kept in the system keychain"
                },
                cx,
            )
            .child(Input::new(&page.key).large()),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .child(
                    Button::new("provider-check")
                        .outline()
                        .label("Check connection")
                        .disabled(checking)
                        .tooltip("Ask the server for its models; nothing is saved")
                        .on_click(cx.listener(|ws, _, _, cx| ws.check_new_provider(cx))),
                )
                .child(
                    Button::new("provider-connect")
                        .primary()
                        .label("Connect")
                        .on_click(cx.listener(|ws, _, window, cx| ws.connect_provider(window, cx))),
                ),
        )
        .children(
            page.form_check
                .as_ref()
                .map(|check| check.line("It answers", cx)),
        )
}

impl Workspace {
    /// Takes the provider's own address into the box, when the kind has one.
    pub(super) fn pick_provider_kind(
        &mut self,
        kind: ProviderKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let page = &mut self.providers_page;
        page.kind = kind;
        page.kinds_open = false;
        page.form_check = None;
        if let Some(url) = kind.base_url() {
            page.url
                .update(cx, |url_box, cx| url_box.set_value(url, window, cx));
        }
        cx.notify();
    }

    /// Asks the server of the form for its models, and shows what came back.
    fn check_new_provider(&mut self, cx: &mut Context<Self>) {
        let page = &mut self.providers_page;
        let kind = page.kind;
        let base_url = page.url.read(cx).value().trim().to_owned();
        let api_key = Some(page.key.read(cx).value().to_string()).filter(|key| !key.is_empty());
        if base_url.is_empty() {
            page.form_check = Some(Check::Done(Err("Give the server's address first.".into())));
            cx.notify();
            return;
        }
        page.form_check = Some(Check::Running);
        let target = ProviderTarget::New {
            kind,
            base_url,
            api_key: api_key.map(dusk_protocol::client::Secret),
        };
        self.core.call(
            cx,
            move |client| async move { client.settings().check_provider(target).await },
            |ws, result, cx| {
                ws.providers_page.form_check = Some(match result {
                    Ok(status) => Check::of(&status),
                    Err(error) => Check::failed(&error),
                });
                cx.notify();
            },
        );
        cx.notify();
    }

    pub(super) fn connect_provider(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let page = &mut self.providers_page;
        let kind = page.kind;
        let name =
            Some(page.name.read(cx).value().trim().to_owned()).filter(|name| !name.is_empty());
        let url = page.url.read(cx).value().trim().to_owned();
        let key = Some(page.key.read(cx).value().to_string()).filter(|key| !key.is_empty());
        for field in [&page.name, &page.url, &page.key] {
            field.update(cx, |field, cx| field.set_value("", window, cx));
        }
        page.form_check = None;
        self.core.call(
            cx,
            move |client| async move { client.settings().connect(kind, name, url, key).await },
            |ws, result, cx| match result {
                Ok(_) => {
                    // Opened over the chat, the form has done its work.
                    if matches!(ws.modal, Some(crate::modal::Modal::ConnectProvider)) {
                        ws.close_modal(cx);
                    }
                    ws.load_models(cx)
                }
                Err(error) => ws.fail(error, cx),
            },
        );
    }
}
