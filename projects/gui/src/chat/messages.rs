//! The transcript drawn: messages as markdown, reasoning folded, pictures in full, and
//! the agent's steps folded so only the latest shows; unfolding the run shows every
//! step, and unfolding a step shows its arguments and answer. The view follows new
//! output only while it is scrolled to the bottom.

use std::path::Path;

use gpui_kit::{
    component::{ActiveTheme, IconName},
    *,
};

use super::{
    links,
    transcript::{Item, Tool, ToolState},
    working,
};
use crate::{widgets, workspace::Workspace};

/// Keys of unfolded things share one set: answers by index, runs and steps apart.
const RUN: usize = 1 << 40;
const STEP: usize = 1 << 41;
/// How close to the bottom still counts as at the bottom.
const FOLLOW_SLACK: f32 = 48.;

pub fn render(ws: &mut Workspace, _: &mut Window, cx: &mut Context<Workspace>) -> Stateful<Div> {
    let shown = ws.shown.clone().unwrap_or_default();
    let Some(chat) = ws.chats.get(&shown) else {
        return div()
            .id("transcript")
            .child(widgets::muted("Opening...", cx));
    };
    let theme = cx.theme();
    let (surface, muted, accent, danger, border) = (
        theme.muted,
        theme.muted_foreground,
        theme.primary,
        theme.danger,
        theme.border,
    );
    let pictures = ws.status.artifacts_dir.clone();
    // Each row is a direct child of the scrolled box, so a sent message can be scrolled to.
    let mut rows: Vec<AnyElement> = Vec::new();
    let mut sent: Vec<(usize, String)> = Vec::new();
    let mut index = 0;
    while index < chat.items.len() {
        let key = format!("{shown}-{index}");
        let item = &chat.items[index];
        if let Item::Tool(_) = item {
            // A run of steps: the latest shows, the rest fold under it.
            let start = index;
            while index < chat.items.len() && matches!(chat.items[index], Item::Tool(_)) {
                index += 1;
            }
            let tools: Vec<&Tool> = chat.items[start..index]
                .iter()
                .filter_map(|item| match item {
                    Item::Tool(tool) => Some(tool),
                    _ => None,
                })
                .collect();
            rows.push(lane(run(ws, &shown, start, &tools, cx).into_any_element()));
            continue;
        }
        index += 1;
        let at = index - 1;
        if let Item::User(text) = item {
            sent.push((rows.len(), text.clone()));
        }
        rows.push(lane(match item {
            Item::User(text) => div()
                .self_end()
                .max_w(px(600.))
                .px_4()
                .py_2()
                .rounded_xl()
                .bg(surface)
                .child(text.clone())
                .into_any_element(),
            Item::Assistant { text, reasoning } => {
                let unfolded = ws.unfolded.contains(&(shown.clone(), at));
                let mut answer = div().flex().flex_col().gap_2();
                if !reasoning.trim().is_empty() {
                    let id = shown.clone();
                    answer = answer.child(
                        div()
                            .id(SharedString::from(format!("reasoning-{key}")))
                            .text_xs()
                            .text_color(muted)
                            .cursor_pointer()
                            .child(if unfolded {
                                "▾ Thinking"
                            } else {
                                "▸ Thinking"
                            })
                            .on_click(cx.listener(move |ws, _, _, cx| ws.fold(&id, at, cx))),
                    );
                    if unfolded {
                        answer = answer.child(
                            div()
                                .pl_3()
                                .border_l_2()
                                .border_color(border)
                                .text_xs()
                                .text_color(muted)
                                .child(reasoning.clone()),
                        );
                    }
                }
                if !text.is_empty() {
                    answer = answer.child(links::markdown(
                        SharedString::from(format!("answer-{key}")),
                        text.clone(),
                    ));
                }
                answer.into_any_element()
            }
            Item::Image(id) => picture(&pictures, id),
            Item::Notice(text) => div()
                .text_xs()
                .text_color(muted)
                .child(text.clone())
                .into_any_element(),
            Item::Error(text) => div()
                .px_3()
                .py_2()
                .rounded_md()
                .text_xs()
                .text_color(danger)
                .bg(danger.opacity(0.1))
                .child(text.clone())
                .into_any_element(),
            Item::Tool(_) => unreachable!("runs are drawn above"),
        }));
    }
    if chat.working {
        rows.push(lane(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_3()
                .text_xs()
                .text_color(muted)
                .child(working::render(
                    SharedString::from(format!("working-{shown}")),
                    accent,
                ))
                .child("Working")
                .into_any_element(),
        ));
    }
    let size = (chat.items.len(), chat.working);
    let at_bottom = {
        let (offset, max) = (ws.scroll.offset().y, ws.scroll.max_offset().y);
        -offset >= max - px(FOLLOW_SLACK)
    };
    if at_bottom && (size != ws.followed || chat.working) {
        ws.scroll.scroll_to_bottom();
    }
    ws.followed = size;
    ws.sent_rows = sent;
    let list = div()
        .id("transcript")
        .size_full()
        .flex()
        .flex_col()
        .gap_4()
        .py_6()
        .overflow_y_scroll()
        .track_scroll(&ws.scroll)
        .children(rows);
    div()
        .id("transcript-frame")
        .relative()
        .size_full()
        .child(list)
}

/// A row at the chat's reading width, centered.
fn lane(content: AnyElement) -> AnyElement {
    div()
        .w_full()
        .flex()
        .justify_center()
        .px_6()
        .child(
            div()
                .w_full()
                .max_w(px(780.))
                .flex()
                .flex_col()
                .child(content),
        )
        .into_any_element()
}

/// A run of steps: the latest one, and the count of the others to unfold.
fn run(
    ws: &Workspace,
    shown: &str,
    start: usize,
    tools: &[&Tool],
    cx: &mut Context<Workspace>,
) -> Div {
    let muted = cx.theme().muted_foreground;
    let open = ws.unfolded.contains(&(shown.to_owned(), start | RUN));
    let mut block = div().flex().flex_col().gap_1();
    if tools.len() > 1 {
        let id = shown.to_owned();
        block = block.child(
            div()
                .id(SharedString::from(format!("run-{shown}-{start}")))
                .text_xs()
                .text_color(muted)
                .cursor_pointer()
                .child(if open {
                    format!("▾ {} steps", tools.len())
                } else {
                    format!("▸ {} earlier steps", tools.len() - 1)
                })
                .on_click(cx.listener(move |ws, _, _, cx| ws.fold(&id, start | RUN, cx))),
        );
    }
    let first = if open { 0 } else { tools.len() - 1 };
    for (offset, tool) in tools.iter().enumerate().skip(first) {
        block = block.child(step(ws, shown, start + offset, tool, cx));
    }
    block
}

fn step(
    ws: &Workspace,
    shown: &str,
    index: usize,
    tool: &Tool,
    cx: &mut Context<Workspace>,
) -> Div {
    let theme = cx.theme();
    let (muted, success, danger, accent, surface) = (
        theme.muted_foreground,
        theme.success,
        theme.danger,
        theme.primary,
        theme.muted,
    );
    let open = ws.unfolded.contains(&(shown.to_owned(), index | STEP));
    let (icon, color) = match tool.state {
        ToolState::Running => (IconName::LoaderCircle, accent),
        ToolState::Done => (IconName::Check, success),
        ToolState::Failed => (IconName::CircleX, danger),
    };
    let id = shown.to_owned();
    let mono = crate::theme::prefs(cx)
        .mono_font
        .clone()
        .unwrap_or_else(|| "Menlo".to_owned());
    let mut line = div().flex().flex_col().child(
        div()
            .id(SharedString::from(format!("step-{shown}-{index}")))
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .text_xs()
            .text_color(muted)
            .cursor_pointer()
            .child(widgets::icon(icon, color))
            .child(
                div()
                    .font_weight(FontWeight::MEDIUM)
                    .child(tool.name.clone()),
            )
            .child(
                div()
                    .truncate()
                    .font_family(mono.clone())
                    .child(tool.detail.clone()),
            )
            .on_click(cx.listener(move |ws, _, _, cx| ws.fold(&id, index | STEP, cx))),
    );
    if open {
        let block = |title: &'static str, text: &str| {
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(div().text_xs().text_color(muted).child(title))
                .child(
                    div()
                        .p_2()
                        .rounded_md()
                        .bg(surface)
                        .text_xs()
                        .font_family(mono.clone())
                        .child(text.to_owned()),
                )
        };
        let mut details = div()
            .flex()
            .flex_col()
            .gap_2()
            .pl_6()
            .pt_1()
            .child(block("Arguments", &tool.arguments));
        if !tool.output.is_empty() {
            details = details.child(block("Answer", &tool.output));
        }
        line = line.child(details);
    }
    line
}

/// A picture from the pictures folder, drawn whole. Only an ID made of hex digits and
/// dashes is taken, so it cannot name a path outside that folder.
fn picture(folder: &Path, id: &str) -> AnyElement {
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_hexdigit() || c == '-') {
        return div().into_any_element();
    }
    img(folder.join(id).join("image"))
        .max_w_full()
        .max_h(px(520.))
        .rounded_lg()
        .object_fit(ObjectFit::Contain)
        .into_any_element()
}

impl Workspace {
    fn fold(&mut self, chat: &str, key: usize, cx: &mut Context<Self>) {
        let key = (chat.to_owned(), key);
        if !self.unfolded.remove(&key) {
            self.unfolded.insert(key);
        }
        cx.notify();
    }
}
