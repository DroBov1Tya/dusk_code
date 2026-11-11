//! The secrets page: which secrets the system keychain holds, never their values. The
//! Hugging Face token is set and cleared here; provider keys go with their providers.

use dusk_protocol::secrets::{HUGGINGFACE, SecretInfo};
use gpui_kit::{
    component::{
        ActiveTheme, Sizable,
        button::{Button, ButtonVariants},
        input::{Input, InputState},
    },
    *,
};

use super::line;
use crate::{widgets, workspace::Workspace};

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
                    .placeholder("hf_... (read access is enough)")
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
    let mut column = div().flex().flex_col();
    column = column.child(
        div().pb_2().text_sm().text_color(muted).child(if info.keychain {
            "Kept in the system keychain. Nothing here shows a value, and none is written to the config file."
        } else {
            "No system keychain was found. Provider keys stay in the private config file, and a token cannot be kept."
        }),
    );
    if let Some(problem) = &info.problem {
        column = column.child(
            div()
                .pb_2()
                .text_sm()
                .text_color(cx.theme().danger)
                .child(format!("The keychain did not answer: {problem}")),
        );
    }
    for entry in &info.entries {
        column = column.child(entry_line(entry, cx));
    }
    if info.keychain {
        column = column
            .child(
                div()
                    .pt_6()
                    .child(widgets::caption("Hugging Face token", cx).px_0()),
            )
            .child(div().text_sm().text_color(muted).child(
                "Used to download speech models, including gated ones, from huggingface.co.",
            ))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap_2()
                    .pt_2()
                    .max_w(px(520.))
                    .child(div().flex_1().child(Input::new(&ws.token_box.input)))
                    .child(
                        Button::new("secret-save")
                            .small()
                            .primary()
                            .label("Save")
                            .on_click(cx.listener(|ws, _, _, cx| ws.save_token(cx))),
                    ),
            );
    }
    column
}

fn entry_line(entry: &SecretInfo, cx: &mut Context<Workspace>) -> Div {
    let name = entry.name.clone();
    let row = line(&entry.label, if entry.set { "Set" } else { "Not set" }, cx);
    if entry.editable && entry.set {
        row.child(
            Button::new(SharedString::from(format!("secret-clear-{name}")))
                .small()
                .ghost()
                .label("Clear")
                .on_click(cx.listener(move |ws, _, _, cx| ws.clear_secret(&name, cx))),
        )
    } else {
        row
    }
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
