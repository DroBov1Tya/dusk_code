//! The secrets page: which secrets the system keychain holds, never their values. The
//! Hugging Face token is set and cleared here; provider keys go with their providers.

use dusk_protocol::config::{HUGGINGFACE, SecretInfo};
use gpui_kit::{
    component::{
        ActiveTheme, Sizable,
        button::{Button, ButtonVariants},
        input::{Input, InputState},
    },
    *,
};

use super::{section, section_hint_color};
use crate::{
    ui::{Type, widgets},
    workspace::Workspace,
};

/// The box the token is typed in, masked.
pub struct TokenBox {
    input: Entity<InputState>,
    /// The token was kept; the box is emptied when next drawn, as that takes the window.
    pub clear: bool,
}

impl TokenBox {
    pub fn new(window: &mut Window, cx: &mut App) -> Self {
        Self {
            input: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder("hf_...")
                    .masked(true)
            }),
            clear: false,
        }
    }

    /// Empties the box when a token was kept.
    pub fn settle(&mut self, window: &mut Window, cx: &mut App) {
        if std::mem::take(&mut self.clear) {
            self.input
                .update(cx, |input, cx| input.set_value("", window, cx));
        }
    }
}

pub fn render(ws: &Workspace, cx: &mut Context<Workspace>) -> Div {
    let muted = cx.theme().muted_foreground;
    let Some(info) = &ws.secrets else {
        return div().child(widgets::muted("Asking the keychain...", cx).px_0());
    };
    let mut column = div().flex().flex_col().gap_4();
    if !info.keychain {
        column = column.child(div().small().text_color(muted).child(
            "No system keychain was found. Provider keys stay in the private config file, and a token cannot be kept.",
        ));
    }
    if let Some(problem) = &info.problem {
        column = column.child(
            div()
                .small()
                .text_color(cx.theme().danger)
                .child(format!("The keychain did not answer: {problem}")),
        );
    }
    let mut entries = section(
        "Stored credentials",
        if info.keychain {
            "Values are hidden and kept out of the config file."
        } else {
            ""
        },
        cx,
    );
    let can_clear = info.entries.iter().any(|entry| entry.editable && entry.set);
    let mut list = div().flex().flex_col().gap_1();
    for entry in &info.entries {
        list = list.child(entry_row(entry, can_clear, cx));
    }
    entries = entries.child(list);
    if info.keychain {
        column = column.child(
            section(
                "Hugging Face token",
                "Download speech models, including gated ones, from huggingface.co. A read token is enough.",
                cx,
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .justify_end()
                    .gap_3()
                    .child(
                        div().flex_1().flex_basis(px(160.)).min_w_0().child(
                            Input::new(&ws.token_box.input)
                                .large()
                                .aria_label("Hugging Face token"),
                        ),
                    )
                    .child(
                        Button::new("secret-save")
                            .large()
                            .h_11()
                            .primary()
                            .label("Save token")
                            .on_click(cx.listener(|ws, _, _, cx| ws.save_token(cx))),
                    ),
            ),
        );
    }
    column.child(entries)
}

fn entry_row(entry: &SecretInfo, can_clear: bool, cx: &mut Context<Workspace>) -> Div {
    let theme = cx.theme();
    let muted = section_hint_color(cx);
    let name = entry.name.clone();
    let status = div()
        .flex()
        .items_center()
        .gap_2()
        .min_w(px(64.))
        .small()
        .text_color(if entry.set { theme.foreground } else { muted })
        .child(
            div()
                .size(px(6.))
                .rounded_full()
                .bg(if entry.set { theme.success } else { muted }),
        )
        .child(if entry.set { "Set" } else { "Not set" });
    let mut actions = div()
        .flex()
        .flex_wrap()
        .items_center()
        .justify_end()
        .gap_2()
        .max_w_full()
        .flex_shrink_0()
        .child(status);
    if can_clear {
        let mut clear = div().w(px(60.)).flex().justify_end();
        if entry.editable && entry.set {
            clear = clear.child(
                Button::new(SharedString::from(format!("secret-clear-{name}")))
                    .ghost()
                    .label("Clear")
                    .on_click(cx.listener(move |ws, _, _, cx| ws.clear_secret(&name, cx))),
            );
        }
        actions = actions.child(clear);
    }
    div()
        .flex()
        .flex_wrap()
        .items_center()
        .justify_end()
        .gap_x_3()
        .gap_y_1()
        .min_h(px(40.))
        .px_2()
        .child(
            div()
                .flex_1()
                .flex_basis(px(150.))
                .min_w_0()
                .body()
                .child(entry.label.clone()),
        )
        .child(actions)
}

impl Workspace {
    pub fn load_secrets(&mut self, cx: &mut Context<Self>) {
        self.core.call(
            cx,
            |client| async move { client.catalog().secrets().await },
            |ws, result, cx| match result {
                Ok(info) => {
                    ws.secrets = Some(info);
                    cx.notify();
                }
                Err(error) => ws.fail(error, cx),
            },
        );
    }

    fn save_token(&mut self, cx: &mut Context<Self>) {
        let token = self.token_box.input.read(cx).value().trim().to_owned();
        if token.is_empty() {
            return;
        }
        self.core.call(
            cx,
            move |client| async move { client.settings().set_secret(HUGGINGFACE, &token).await },
            |ws, result, cx| match result {
                // The value leaves the box once it is kept, and is never shown again.
                Ok(()) => {
                    ws.token_box.clear = true;
                    ws.load_secrets(cx);
                }
                Err(error) => ws.fail(error, cx),
            },
        );
    }

    fn clear_secret(&mut self, name: &str, cx: &mut Context<Self>) {
        let name = name.to_owned();
        self.core.call(
            cx,
            move |client| async move { client.settings().delete_secret(&name).await },
            |ws, result, cx| match result {
                Ok(()) => ws.load_secrets(cx),
                Err(error) => ws.fail(error, cx),
            },
        );
    }
}
