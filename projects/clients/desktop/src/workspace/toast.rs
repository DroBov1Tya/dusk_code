//! A short note over any page that goes away on its own: information, not an error.
//! Errors stay in `notice`, shown until they are dismissed.

use std::time::{Duration, Instant};

use gpui_kit::{component::ActiveTheme, *};

use super::Workspace;
use crate::ui::Type;

/// How long a note shows.
const SHOWN: Duration = Duration::from_secs(5);

impl Workspace {
    /// Shows a note for a few seconds; a newer note takes its place and its time.
    pub fn toast(&mut self, text: String, cx: &mut Context<Self>) {
        let since = Instant::now();
        self.toast = Some((since, text));
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SHOWN).await;
            let _ = this.update(cx, |ws, cx| {
                if ws.toast.as_ref().is_some_and(|(shown, _)| *shown == since) {
                    ws.toast = None;
                    cx.notify();
                }
            });
        })
        .detach();
    }
}

/// The note under the header, clear of the message box, while there is one; a click puts
/// it away.
pub fn render(ws: &Workspace, cx: &mut Context<Workspace>) -> Option<Div> {
    let (_, text) = ws.toast.as_ref()?;
    let theme = cx.theme();
    Some(
        div()
            .absolute()
            .top(px(crate::columns::topbar::HEIGHT + 12.))
            .left_0()
            .right_0()
            .flex()
            .justify_center()
            .child(
                div()
                    .id("toast")
                    .max_w(px(480.))
                    .px_3()
                    .py_2()
                    .rounded_lg()
                    .border_1()
                    .border_color(theme.border)
                    .bg(theme.popover)
                    .text_color(theme.popover_foreground)
                    .shadow_lg()
                    .meta()
                    .cursor_pointer()
                    .child(text.clone())
                    .on_click(cx.listener(|ws, _, _, cx| {
                        ws.toast = None;
                        cx.notify();
                    })),
            ),
    )
}
