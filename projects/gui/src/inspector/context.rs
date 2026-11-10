//! What the shown chat's next request carries, as one bar in colored parts and a row
//! per part: its tokens and its share of the window, then the free space.

use dusk_protocol::{
    command::{Answer, Command},
    context::ContextSection,
};
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
                    .child(widgets::short(tokens)),
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
                    widgets::short(report.total),
                    widgets::short(report.limit)
                ))),
        )
        .child(bar);
    for (index, section) in parts.iter().enumerate() {
        let name = section.name.clone();
        let open = ws.context_open.contains(&name);
        column = column.child(
            row(color(index), section.name.clone(), section.tokens)
                .id(("context-part", index))
                .cursor_pointer()
                .on_click(cx.listener(move |ws, _, _, cx| {
                    if !ws.context_open.remove(&name) {
                        ws.context_open.insert(name.clone());
                    }
                    cx.notify();
                })),
        );
        if open {
            column = column.child(items(section, muted, cx));
        }
    }
    column.child(row(track, "Free space".to_owned(), report.limit - used))
}

/// What a part is made of, largest first; skills and MCP tools also say how often
/// they were used, here and in every chat.
fn items(section: &ContextSection, muted: Hsla, cx: &App) -> Div {
    let mut list = div().flex().flex_col().gap_1().pl_5().pb_1().text_xs();
    if section.items.is_empty() {
        return list.child(div().text_color(muted).child("nothing"));
    }
    for (name, tokens) in &section.items {
        let uses = section
            .details
            .iter()
            .find(|detail| &detail.name == name)
            .map(|detail| {
                format!(
                    "used {} here, {} in all",
                    detail.uses_in_chat, detail.uses_total
                )
            });
        list = list.child(
            div()
                .flex()
                .flex_row()
                .gap_2()
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_w_0()
                        .child(div().truncate().child(name.clone()))
                        .children(uses.map(|uses| div().text_color(muted).child(uses))),
                )
                .child(
                    div()
                        .text_color(cx.theme().muted_foreground)
                        .child(widgets::short(*tokens)),
                ),
        );
    }
    list
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
