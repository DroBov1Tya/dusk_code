//! The message box: Enter sends, Shift+Enter starts a new line, Stop ends the turn.

use gpui_kit::{
    component::{
        ActiveTheme, IconName, Sizable,
        button::{Button, ButtonRounded, ButtonVariants},
        input::Textarea,
    },
    *,
};

use super::transcript::Transcript;
use crate::workspace::Workspace;

pub fn render(ws: &Workspace, _: &mut Window, cx: &mut Context<Workspace>) -> Div {
    let working = ws.shown_chat().is_some_and(|chat| chat.working);
    let button = if working {
        Button::new("stop")
            .danger()
            .small()
            .rounded(ButtonRounded::Size(px(999.)))
            .icon(IconName::Square)
            .tooltip("Stop")
            .on_click(cx.listener(|ws, _, _, cx| ws.cancel(cx)))
    } else {
        Button::new("send")
            .primary()
            .small()
            .rounded(ButtonRounded::Size(px(999.)))
            .icon(IconName::ArrowUp)
            .tooltip("Send")
            .on_click(cx.listener(|ws, _, window, cx| ws.send(window, cx)))
    };
    let attach = ws.attach_open.then(|| attach_menu(cx));
    div()
        .relative()
        .px_6()
        .pt_2()
        .max_w(px(780.))
        .mx_auto()
        .w_full()
        .child(
            div()
                .flex()
                .flex_row()
                .items_end()
                .gap_2()
                .p_2()
                .rounded_xl()
                .border_1()
                .border_color(cx.theme().border)
                .bg(cx.theme().muted)
                .child(
                    Button::new("attach")
                        .ghost()
                        .small()
                        .rounded(ButtonRounded::Size(px(999.)))
                        .icon(IconName::Plus)
                        .tooltip("Attach files or folders")
                        .on_click(cx.listener(|ws, _, _, cx| {
                            ws.attach_open = !ws.attach_open;
                            cx.notify();
                        })),
                )
                .child(
                    div()
                        .flex_1()
                        .child(Textarea::new(&ws.composer).appearance(false)),
                )
                .child(button),
        )
        .children(attach)
        // Tab switches the mode, before the text box takes it as an indent.
        .capture_key_down(cx.listener(|ws, event: &KeyDownEvent, _, cx| {
            let keys = &event.keystroke;
            if keys.key == "tab" && !keys.modifiers.shift && !keys.modifiers.control {
                cx.stop_propagation();
                ws.next_mode(cx);
            }
        }))
}

/// Files first, then folders; each opens the system's own picker.
fn attach_menu(cx: &mut Context<Workspace>) -> Stateful<Div> {
    let theme = cx.theme();
    div()
        .id("attach-menu")
        .absolute()
        .bottom(px(64.))
        .left(px(24.))
        .w(px(220.))
        .p_1()
        .flex()
        .flex_col()
        .rounded_xl()
        .border_1()
        .border_color(theme.border)
        .bg(theme.popover)
        .shadow_lg()
        .child(
            crate::widgets::row("attach-files", false, cx)
                .child(crate::widgets::icon(IconName::File, theme.muted_foreground))
                .child("Files")
                .on_click(cx.listener(|ws, _, window, cx| ws.attach(false, window, cx))),
        )
        .child(
            crate::widgets::row("attach-folders", false, cx)
                .child(crate::widgets::icon(
                    IconName::Folder,
                    theme.muted_foreground,
                ))
                .child("Folders")
                .on_click(cx.listener(|ws, _, window, cx| ws.attach(true, window, cx))),
        )
        .on_mouse_down_out(cx.listener(|ws, _, _, cx| {
            ws.attach_open = false;
            cx.notify();
        }))
}

impl Workspace {
    /// Sends the composer's text to the shown chat, or to a new chat of the selected
    /// project when none is shown.
    pub fn send(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mut text = self.composer.read(cx).value().trim().to_owned();
        if text.is_empty() {
            return;
        }
        let shown = self.shown.clone();
        // A pending question takes the text as the user's own answer.
        let question = shown
            .as_ref()
            .and_then(|id| self.chats.get_mut(id))
            .and_then(|chat| chat.question.take());
        if let Some(question) = question {
            text = question.answer(&[], &text);
        }
        // Without a project, a new chat works in the workspace root.
        let dir = Some(
            self.selected_project()
                .map_or_else(|| self.home.clone(), |project| project.dir.clone()),
        );
        self.composer
            .update(cx, |composer, cx| composer.set_value("", window, cx));
        self.core.call(
            cx,
            move |client| async move {
                let threads = client.threads();
                let (thread_id, new) = match (shown, dir) {
                    (Some(thread_id), _) => (thread_id, false),
                    (None, Some(dir)) => (threads.start(dir).await?, true),
                    (None, None) => anyhow::bail!("no project"),
                };
                threads.send(&thread_id, text, false).await?;
                anyhow::Ok((thread_id, new))
            },
            |ws, result, cx| match result {
                Ok((thread_id, new)) => {
                    if new {
                        ws.chats.insert(thread_id.clone(), Transcript::default());
                        ws.show(thread_id, cx);
                        ws.reload_threads(cx);
                    }
                }
                Err(error) => ws.fail(error, cx),
            },
        );
    }

    /// Puts the picked paths into the message; the core keeps named pictures and files
    /// among the chat's sources when it is sent.
    fn attach(&mut self, folders: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.attach_open = false;
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: !folders,
            directories: folders,
            multiple: true,
            prompt: Some("Attach".into()),
        });
        cx.spawn_in(window, async move |ws, cx| {
            let Ok(Ok(Some(paths))) = picked.await else {
                return;
            };
            let _ = ws.update_in(cx, |ws, window, cx| {
                let mut text = ws.composer.read(cx).value().to_string();
                for path in paths {
                    if !text.is_empty() && !text.ends_with(char::is_whitespace) {
                        text.push(' ');
                    }
                    text.push_str(&path.display().to_string());
                }
                ws.composer
                    .update(cx, |composer, cx| composer.set_value(text, window, cx));
            });
        })
        .detach();
        cx.notify();
    }

    fn cancel(&mut self, cx: &mut Context<Self>) {
        let Some(thread_id) = self.shown.clone() else {
            return;
        };
        self.core.call(
            cx,
            move |client| async move { client.threads().cancel(&thread_id).await },
            |ws, result, cx| {
                if let Err(error) = result {
                    ws.fail(error, cx);
                }
            },
        );
    }
}
