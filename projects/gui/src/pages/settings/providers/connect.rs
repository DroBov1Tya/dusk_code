//! The card that connects a provider: its kind, its address and key, and a check that finds
//! out whether it answers before anything is saved.

use dusk_protocol::{client::ProviderTarget, config::ProviderKind};
use gpui_kit::{
    component::{
        ActiveTheme, Disableable, Sizable,
        button::{Button, ButtonVariants},
        input::Input,
    },
    prelude::FluentBuilder,
    *,
};

use super::{card, check::Check, labeled};
use crate::{ui::Type, workspace::Workspace};

const KINDS: [(ProviderKind, &str, &str); 3] = [
    (
        ProviderKind::OpenAiCompatible,
        "Custom",
        "Any server with the OpenAI API",
    ),
    (ProviderKind::OpenAi, "OpenAI", "api.openai.com"),
    (ProviderKind::Anthropic, "Anthropic", "api.anthropic.com"),
];

pub fn render(ws: &Workspace, cx: &mut Context<Workspace>) -> Div {
    let page = &ws.providers_page;
    let (muted, accent, border) = {
        let theme = cx.theme();
        (theme.muted_foreground, theme.primary, theme.border)
    };
    let mut kinds = div().flex().flex_row().gap_2();
    for (kind, label, about) in KINDS {
        let chosen = page.kind == kind;
        kinds = kinds.child(
            div()
                .id(("provider-kind", kind as usize))
                .flex_1()
                .flex()
                .flex_col()
                .gap_0p5()
                .px_3()
                .py_2()
                .rounded_lg()
                .border_1()
                .border_color(if chosen { accent } else { border })
                .when(chosen, |tile| tile.bg(accent.opacity(0.10)))
                .cursor_pointer()
                .child(div().body().font_weight(FontWeight::MEDIUM).child(label))
                .child(div().meta().text_color(muted).child(about))
                .on_click(
                    cx.listener(move |ws, _, window, cx| ws.pick_provider_kind(kind, window, cx)),
                ),
        );
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
        .child(kinds)
        .child(labeled("Address", "where the server is", cx).child(Input::new(&page.url).large()))
        .child(
            labeled("API key", "kept in the system keychain", cx)
                .child(Input::new(&page.key).large()),
        )
        .child(
            labeled("Name", "optional; it shows in the model menu", cx)
                .child(Input::new(&page.name).large()),
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
                Ok(_) => ws.load_models(cx),
                Err(error) => ws.fail(error, cx),
            },
        );
    }
}
