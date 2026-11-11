//! What the one button that forgets every provider says before it does.

use gpui_kit::{
    component::{
        ActiveTheme,
        button::{Button, ButtonVariants},
    },
    *,
};

use crate::workspace::Workspace;

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

impl Workspace {
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
}
