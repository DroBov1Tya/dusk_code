//! The transcript drawn as a virtual list: only the rows in sight are laid out, their
//! heights cached, so a long chat scrolls without lag. Messages are markdown, reasoning
//! folds, pictures are cards, and the agent's steps fold so only the latest shows. The
//! list follows new output while it is scrolled to the end.

use std::path::Path;

use gpui_kit::{
    component::{ActiveTheme, IconName},
    *,
};

use super::{
    links,
    transcript::{Item, Picture, Tool, ToolState},
    working,
};
use crate::{widgets, workspace::Workspace};

/// Keys of unfolded things share one set: answers by index, runs and steps apart.
const RUN: usize = 1 << 40;
const STEP: usize = 1 << 41;

/// One row of the list: an item, a run of steps, or the working figure.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Row {
    Item(usize),
    Run { start: usize, end: usize },
    Working,
}

pub fn render(ws: &mut Workspace, _: &mut Window, cx: &mut Context<Workspace>) -> AnyElement {
    let shown = ws.shown.clone().unwrap_or_default();
    let Some(chat) = ws.chats.get(&shown) else {
        return widgets::muted("Opening...", cx).into_any_element();
    };
    let mut rows = Vec::new();
    let mut sent = Vec::new();
    let mut index = 0;
    while index < chat.items.len() {
        if let Item::Tool(_) = chat.items[index] {
            let start = index;
            while index < chat.items.len() && matches!(chat.items[index], Item::Tool(_)) {
                index += 1;
            }
            if !steps(&chat.items[start..index]).is_empty() {
                rows.push(Row::Run { start, end: index });
            }
            continue;
        }
        if let Item::User(text) = &chat.items[index] {
            sent.push((rows.len(), text.clone()));
        }
        rows.push(Row::Item(index));
        index += 1;
    }
    if chat.working {
        rows.push(Row::Working);
    }
    sync(ws, &shown, &rows);
    ws.rows = rows;
    ws.sent_rows = sent;
    list(
        ws.list.clone(),
        cx.processor(|ws, ix: usize, _window, cx| ws.transcript_row(ix, cx)),
    )
    .size_full()
    .into_any_element()
}

/// Tells the list what changed: a new chat is measured anew; rows that came or changed
/// at the end, where streaming text grows, are measured again.
fn sync(ws: &mut Workspace, shown: &str, rows: &[Row]) {
    if ws.list_chat.as_deref() != Some(shown) {
        ws.list_chat = Some(shown.to_owned());
        ws.list.reset(rows.len());
        ws.list.set_follow_mode(FollowMode::Tail);
        return;
    }
    let old = ws.list.item_count();
    let same = old.min(rows.len());
    // The first row that differs from what the list holds, and the last one, which may
    // still be growing.
    let first = ws
        .rows
        .iter()
        .zip(rows)
        .position(|(a, b)| a != b)
        .unwrap_or(same)
        .min(same.saturating_sub(1));
    if old != rows.len() || ws.measure_again {
        ws.list.splice(first..old, rows.len() - first);
    } else if let Some(last) = rows.len().checked_sub(1) {
        ws.list.splice(last..last + 1, 1);
    }
    if ws.measure_again {
        ws.measure_again = false;
        ws.list.splice(0..rows.len(), rows.len());
    }
}

/// The steps a run shows: a picture card stands for the step that showed it.
fn steps(items: &[Item]) -> Vec<&Tool> {
    items
        .iter()
        .filter_map(|item| match item {
            Item::Tool(tool)
                if !(tool.name == "display_image" && tool.state == ToolState::Done) =>
            {
                Some(tool)
            }
            _ => None,
        })
        .collect()
}

impl Workspace {
    fn transcript_row(&mut self, ix: usize, cx: &mut Context<Self>) -> AnyElement {
        let shown = self.shown.clone().unwrap_or_default();
        let (Some(row), Some(chat)) = (self.rows.get(ix).copied(), self.chats.get(&shown)) else {
            return div().into_any_element();
        };
        let theme = cx.theme();
        let (surface, muted, accent, danger, border) = (
            theme.muted,
            theme.muted_foreground,
            theme.primary,
            theme.danger,
            theme.border,
        );
        // Space between rows lives in the rows, as the list has no gap of its own.
        let content = match row {
            Row::Working => div()
                .flex()
                .flex_row()
                .items_center()
                .gap_3()
                .text_xs()
                .text_color(muted)
                .child(working::render(
                    SharedString::from(format!("working-{shown}")),
                    accent,
                    theme.foreground,
                ))
                .child("Working")
                .into_any_element(),
            Row::Run { start, end } => {
                let tools = steps(&chat.items[start..end]);
                run(self, &shown, start, &tools, cx).into_any_element()
            }
            Row::Item(at) => {
                let key = format!("{shown}-{at}");
                match &chat.items[at] {
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
                        let unfolded = self.unfolded.contains(&(shown.clone(), at));
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
                                    .on_click(
                                        cx.listener(move |ws, _, _, cx| ws.fold(&id, at, cx)),
                                    ),
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
                            answer = answer.child(super::media::render(
                                &format!("answer-{key}"),
                                text,
                                cx,
                            ));
                        }
                        answer.into_any_element()
                    }
                    Item::Image(image) => picture(&self.status.artifacts_dir, image, cx),
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
                    Item::Tool(_) => div().into_any_element(),
                }
            }
        };
        let top = if ix == 0 { 24. } else { 8. };
        let bottom = if ix + 1 == self.rows.len() { 24. } else { 8. };
        div()
            .pt(px(top))
            .pb(px(bottom))
            .child(lane(content))
            .into_any_element()
    }
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

/// A picture from the pictures folder as a card: the picture on the left, its title and
/// description beside it, a hairline under it to part it from the next. Only an ID of
/// hex digits and dashes is taken, so it cannot name a path outside that folder.
fn picture(folder: &Path, image: &Picture, cx: &App) -> AnyElement {
    let id = &image.id;
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_hexdigit() || c == '-') {
        return div().into_any_element();
    }
    let theme = cx.theme();
    let (border, muted, surface) = (theme.border, theme.muted_foreground, theme.muted);
    let mut text = div().flex().flex_col().gap_2().flex_1().min_w_0();
    if let Some(title) = &image.title {
        text = text.child(
            div()
                .text_lg()
                .font_weight(FontWeight::SEMIBOLD)
                .child(title.clone()),
        );
    }
    if let Some(description) = &image.description {
        text = text.child(links::markdown(
            SharedString::from(format!("picture-{id}")),
            description.clone(),
        ));
    }
    if image.title.is_none() && image.description.is_none() {
        text = text.child(div().text_xs().text_color(muted).child("Picture"));
    }
    div()
        .flex()
        .flex_row()
        .gap_5()
        .pb_4()
        .border_b_1()
        .border_color(border)
        .child(
            div()
                .w(px(220.))
                .flex_none()
                .rounded_xl()
                .overflow_hidden()
                .bg(surface)
                .child(
                    img(folder.join(id).join("image"))
                        .w_full()
                        .max_h(px(320.))
                        .object_fit(ObjectFit::Cover),
                ),
        )
        .child(text)
        .into_any_element()
}

impl Workspace {
    fn fold(&mut self, chat: &str, key: usize, cx: &mut Context<Self>) {
        let key = (chat.to_owned(), key);
        if !self.unfolded.remove(&key) {
            self.unfolded.insert(key);
        }
        self.measure_again = true;
        cx.notify();
    }
}
