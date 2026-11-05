//! What a right click on a chat offers: rename, pin to the top, delete. Delete asks
//! first, since the core erases the chat and its folder for good.

use gpui_kit::{
    component::{
        ActiveTheme, IconName,
        button::{Button, ButtonVariants},
        input::{Input, InputState},
    },
    *,
};

use super::Modal;
use crate::{theme, widgets, workspace::Workspace};

/// The small menu at the pointer, over everything.
pub fn render(ws: &Workspace, cx: &mut Context<Workspace>) -> Option<Stateful<Div>> {
    let (thread_id, at) = ws.chat_menu.clone()?;
    let theme = cx.theme();
    let muted = theme.muted_foreground;
    let pinned = theme::prefs(cx).pinned.contains(&thread_id);
    let (rename, pin, delete) = (thread_id.clone(), thread_id.clone(), thread_id);
    Some(
        div()
            .id("chat-menu")
            .absolute()
            .left(at.x)
            .top(at.y)
            .w(px(200.))
            .p_1()
            .flex()
            .flex_col()
            .rounded_lg()
            .border_1()
            .border_color(theme.border)
            .bg(theme.popover)
            .shadow_lg()
            .child(
                widgets::row("menu-rename", false, cx)
                    .child(widgets::icon(IconName::Replace, muted))
                    .child("Rename")
                    .on_click(
                        cx.listener(move |ws, _, window, cx| ws.ask_rename(&rename, window, cx)),
                    ),
            )
            .child(
                widgets::row("menu-pin", false, cx)
                    .child(widgets::icon(IconName::Star, muted))
                    .child(if pinned { "Unpin" } else { "Pin to the top" })
                    .on_click(cx.listener(move |ws, _, _, cx| ws.toggle_pin(&pin, cx))),
            )
            .child(
                widgets::row("menu-delete", false, cx)
                    .text_color(theme.danger)
                    .child(widgets::icon(IconName::Delete, theme.danger))
                    .child("Delete")
                    .on_click(cx.listener(move |ws, _, _, cx| ws.ask_delete(&delete, cx))),
            )
            .on_mouse_down_out(cx.listener(|ws, _, _, cx| {
                ws.chat_menu = None;
                cx.notify();
            })),
    )
}

pub fn rename(title: &Entity<InputState>, cx: &mut Context<Workspace>) -> Div {
    div()
        .flex()
        .flex_col()
        .gap_3()
        .p_5()
        .child(Input::new(title))
        .child(
            div()
                .flex()
                .flex_row()
                .justify_end()
                .gap_2()
                .child(
                    Button::new("rename-cancel")
                        .ghost()
                        .label("Cancel")
                        .on_click(cx.listener(|ws, _, _, cx| ws.close_modal(cx))),
                )
                .child(
                    Button::new("rename-save")
                        .primary()
                        .label("Rename")
                        .on_click(cx.listener(|ws, _, _, cx| ws.rename_chat(cx))),
                ),
        )
}

pub fn delete(title: &str, cx: &mut Context<Workspace>) -> Div {
    let muted = cx.theme().muted_foreground;
    div()
        .flex()
        .flex_col()
        .gap_3()
        .p_5()
        .child(format!("Delete \"{title}\"?"))
        .child(
            div()
                .text_sm()
                .text_color(muted)
                .child("Its messages, its agents' threads and its folder in the workspace are erased. This cannot be undone."),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .justify_end()
                .gap_2()
                .child(
                    Button::new("delete-cancel")
                        .ghost()
                        .label("Keep it")
                        .on_click(cx.listener(|ws, _, _, cx| ws.close_modal(cx))),
                )
                .child(
                    Button::new("delete-confirm")
                        .danger()
                        .label("Delete for good")
                        .on_click(cx.listener(|ws, _, _, cx| ws.delete_chat(cx))),
                ),
        )
}

impl Workspace {
    fn title_of(&self, thread_id: &str) -> String {
        self.threads
            .iter()
            .find(|thread| thread.id == thread_id)
            .and_then(|thread| thread.title.clone())
            .unwrap_or_else(|| "New chat".to_owned())
    }

    fn ask_rename(&mut self, thread_id: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.chat_menu = None;
        let current = self.title_of(thread_id);
        let title = cx.new(|cx| {
            let mut state = InputState::new(window, cx);
            state.set_value(current, window, cx);
            state
        });
        self.modal = Some(Modal::Rename {
            thread_id: thread_id.to_owned(),
            title,
        });
        cx.notify();
    }

    fn rename_chat(&mut self, cx: &mut Context<Self>) {
        let Some(Modal::Rename { thread_id, title }) = &self.modal else {
            return;
        };
        let (thread_id, title) = (thread_id.clone(), title.read(cx).value().trim().to_owned());
        if title.is_empty() {
            return;
        }
        let shown = title.clone();
        let id = thread_id.clone();
        self.core.call(
            cx,
            move |client| async move { client.threads().rename(&thread_id, &title).await },
            move |ws, result, cx| match result {
                Ok(()) => {
                    if let Some(chat) = ws.chats.get_mut(&id) {
                        chat.title = Some(shown);
                    }
                    ws.modal = None;
                    ws.reload_threads(cx);
                }
                Err(error) => ws.fail(error, cx),
            },
        );
    }

    fn toggle_pin(&mut self, thread_id: &str, cx: &mut Context<Self>) {
        self.chat_menu = None;
        let thread_id = thread_id.to_owned();
        theme::change(cx, |prefs| {
            if let Some(at) = prefs.pinned.iter().position(|id| *id == thread_id) {
                prefs.pinned.remove(at);
            } else {
                prefs.pinned.insert(0, thread_id);
            }
        });
        cx.notify();
    }

    fn ask_delete(&mut self, thread_id: &str, cx: &mut Context<Self>) {
        self.chat_menu = None;
        self.modal = Some(Modal::Delete {
            thread_id: thread_id.to_owned(),
            title: self.title_of(thread_id),
        });
        cx.notify();
    }

    fn delete_chat(&mut self, cx: &mut Context<Self>) {
        let Some(Modal::Delete { thread_id, .. }) = &self.modal else {
            return;
        };
        let thread_id = thread_id.clone();
        let id = thread_id.clone();
        self.core.call(
            cx,
            move |client| async move { client.threads().delete(&thread_id).await },
            move |ws, result, cx| match result {
                Ok(()) => {
                    ws.chats.remove(&id);
                    if ws.shown.as_deref() == Some(id.as_str()) {
                        ws.shown = None;
                    }
                    theme::change(cx, |prefs| prefs.pinned.retain(|pinned| *pinned != id));
                    ws.modal = None;
                    ws.reload_threads(cx);
                }
                Err(error) => ws.fail(error, cx),
            },
        );
    }
}
