//! What the shown chat's next request carries, section by section.

use dusk_protocol::command::{Answer, Command};
use gpui_kit::{component::ActiveTheme, *};

use crate::{widgets, workspace::Workspace};

pub fn render(ws: &Workspace, cx: &mut Context<Workspace>) -> Div {
    let Some(report) = &ws.context else {
        return div().child(widgets::muted("Open a chat to see its context.", cx));
    };
    let theme = cx.theme();
    let (accent, track, muted) = (theme.primary, theme.border, theme.muted_foreground);
    let mut column = div().flex().flex_col().gap_2().p_3().child(
        div()
            .font_weight(FontWeight::SEMIBOLD)
            .child(format!("{} of {} tokens", report.total, report.limit)),
    );
    for section in &report.sections {
        let share = if report.limit == 0 {
            0.
        } else {
            (section.tokens as f32 / report.limit as f32).min(1.)
        };
        column = column.child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .justify_between()
                        .text_xs()
                        .child(section.name.clone())
                        .child(div().text_color(muted).child(section.tokens.to_string())),
                )
                .child(
                    div()
                        .h(px(4.))
                        .rounded_full()
                        .bg(track)
                        .child(div().h_full().rounded_full().bg(accent).w(relative(share))),
                ),
        );
    }
    column
}

impl Workspace {
    pub fn load_context(&mut self, cx: &mut Context<Self>) {
        let Some(thread_id) = self.shown.clone() else {
            return;
        };
        self.core.call(
            cx,
            move |client| async move {
                let thread_id = Some(thread_id);
                client.command(Command::Context { thread_id }).await
            },
            |ws, result, cx| match result {
                Ok(Answer::Context(report)) => {
                    ws.context = Some(report);
                    cx.notify();
                }
                Ok(_) => {}
                Err(error) => ws.fail(error, cx),
            },
        );
    }
}
