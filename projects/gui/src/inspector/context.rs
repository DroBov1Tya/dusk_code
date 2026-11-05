//! What the shown chat's next request carries: how full the context is, and each kind
//! of load with its share of the window.

use dusk_protocol::command::{Answer, Command};
use gpui_kit::{component::ActiveTheme, *};

use crate::{widgets, workspace::Workspace};

pub fn render(ws: &Workspace, cx: &mut Context<Workspace>) -> Div {
    let Some(report) = &ws.context else {
        return div().child(widgets::muted("Open a chat to see its context.", cx));
    };
    let theme = cx.theme();
    let (accent, track, muted, warning, danger) =
        (theme.primary, theme.border, theme.muted_foreground, theme.warning, theme.danger);
    let share = |tokens: u64| {
        if report.limit == 0 {
            0.
        } else {
            (tokens as f32 / report.limit as f32).min(1.)
        }
    };
    let filled = share(report.total);
    let fill = if filled > 0.9 {
        danger
    } else if filled > 0.7 {
        warning
    } else {
        accent
    };
    let bar = |part: f32, color: Hsla, height: f32| {
        div()
            .h(px(height))
            .rounded_full()
            .bg(track)
            .child(div().h_full().rounded_full().bg(color).w(relative(part)))
    };
    let mut column = div()
        .flex()
        .flex_col()
        .gap_3()
        .px_4()
        .child(
            div()
                .flex()
                .flex_row()
                .justify_between()
                .text_sm()
                .child(format!("{:.0}% full", filled * 100.))
                .child(
                    div()
                        .text_color(muted)
                        .child(format!("{} / {}", report.total, report.limit)),
                ),
        )
        .child(bar(filled, fill, 6.));
    for section in &report.sections {
        let part = share(section.tokens);
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
                        .child(
                            div()
                                .text_color(muted)
                                .child(format!("{:.1}% · {}", part * 100., section.tokens)),
                        ),
                )
                .child(bar(part, muted, 3.)),
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
