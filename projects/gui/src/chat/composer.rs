//! The message box: Enter sends, Shift+Enter starts a new line, Stop ends the turn.

use gpui_kit::{
    component::{
        ActiveTheme, IconName, Sizable,
        button::{Button, ButtonVariants},
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
            .icon(IconName::Square)
            .tooltip("Stop")
            .on_click(cx.listener(|ws, _, _, cx| ws.cancel(cx)))
    } else {
        Button::new("send")
            .primary()
            .small()
            .icon(IconName::ArrowUp)
            .tooltip("Send")
            .on_click(cx.listener(|ws, _, window, cx| ws.send(window, cx)))
    };
    div()
        .px_6()
        .pt_2()
        .max_w(px(900.))
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
                .child(div().flex_1().child(Textarea::new(&ws.composer).appearance(false)))
                .child(button),
        )
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
