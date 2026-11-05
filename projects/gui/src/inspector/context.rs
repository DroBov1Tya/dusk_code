//! What the shown chat's next request carries, as one bar in colored parts and a row
//! per part: its tokens and its share of the window, then the free space.

use dusk_protocol::command::{Answer, Command};
use gpui_kit::{component::ActiveTheme, *};

use crate::{widgets, workspace::Workspace};

pub fn render(ws: &Workspace, cx: &mut Context<Workspace>) -> Div {
    let Some(report) = &ws.context else {
        return div().child(widgets::muted("Open a chat to see its context.", cx));
    };
    let theme = cx.theme();
    let (muted, track, foreground) = (theme.muted_foreground, theme.border, theme.foreground);
    // One color per part, the largest first; the rest share a quiet grey.
    let colors = [
        theme.primary,
        theme.cyan,
        theme.success,
        theme.warning,
        theme.magenta,
        theme.blue,
    ];
    let limit = report.limit.max(1) as f32;
    let share = |tokens: u64| (tokens as f32 / limit).min(1.);
    let mut parts: Vec<_> = report.sections.iter().collect();
    parts.sort_by_key(|section| std::cmp::Reverse(section.tokens));
    let color = |index: usize| colors.get(index).copied().unwrap_or(muted.opacity(0.6));
    let mut bar = div()
        .flex()
        .flex_row()
        .h(px(8.))
        .rounded_full()
        .overflow_hidden()
        .bg(track);
    for (index, section) in parts.iter().enumerate() {
        bar = bar.child(
            div()
                .h_full()
                .w(relative(share(section.tokens)))
                .bg(color(index)),
        );
    }
    let row = |swatch: Hsla, name: String, tokens: u64| {
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .text_sm()
            .child(div().size(px(10.)).rounded_sm().bg(swatch))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_color(foreground.opacity(0.85))
                    .child(name),
            )
            .child(
                div()
                    .w(px(56.))
                    .text_right()
                    .text_color(muted)
                    .child(short(tokens)),
            )
            .child(
                div()
                    .w(px(48.))
                    .text_right()
                    .child(format!("{:.1}%", share(tokens) * 100.)),
            )
    };
    let used = report.total.min(report.limit);
    let mut column = div()
        .flex()
        .flex_col()
        .gap_2()
        .px_4()
        .child(
            div()
                .flex()
                .flex_row()
                .justify_between()
                .child(format!("{:.0}% used", share(report.total) * 100.))
                .child(div().text_color(muted).child(format!(
                    "{} / {}",
                    short(report.total),
                    short(report.limit)
                ))),
        )
        .child(bar);
    for (index, section) in parts.iter().enumerate() {
        column = column.child(row(color(index), section.name.clone(), section.tokens));
    }
    column.child(row(track, "Free space".to_owned(), report.limit - used))
}

fn short(tokens: u64) -> String {
    match tokens {
        0..1_000 => tokens.to_string(),
        1_000..1_000_000 => format!("{:.1}k", tokens as f64 / 1e3),
        _ => format!("{:.1}M", tokens as f64 / 1e6),
    }
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
