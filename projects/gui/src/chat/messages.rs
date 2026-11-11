//! The transcript drawn as a virtual list: only the rows in sight are laid out, their
//! heights cached, so a long chat scrolls without lag. Messages are markdown, reasoning
//! folds, pictures are cards, and the agent's steps fold so only the latest shows. The
//! list follows new output while it is scrolled to the end.

use std::path::Path;

use gpui_kit::{
    component::{
        ActiveTheme, Disableable, IconName, Sizable,
        button::{Button, ButtonVariants},
    },
    prelude::FluentBuilder,
    *,
};

use super::{
    links,
    transcript::{CommandRun, Item, Picture, Tool, ToolState},
    working,
};
use crate::{ui::Type, widgets, workspace::Workspace};

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
        if let Item::User { text, .. } = &chat.items[index] {
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
                if !(matches!(tool.name.as_str(), "display_image" | "display_audio")
                    && tool.state == ToolState::Done) =>
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
                .meta()
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
                    Item::User { text, sequence } => {
                        let group = SharedString::from(format!("user-row-{key}"));
                        let mut tools = div()
                            .flex()
                            .flex_row()
                            .gap_1()
                            .invisible()
                            .group_hover(group.clone(), |tools| tools.visible());
                        let copied = text.clone();
                        tools = tools.child(
                            Button::new(SharedString::from(format!("copy-{key}")))
                                .ghost()
                                .xsmall()
                                .icon(IconName::Copy)
                                .tooltip("Copy the message")
                                .on_click(cx.listener(move |_, _, _, cx| {
                                    cx.write_to_clipboard(ClipboardItem::new_string(copied.clone()))
                                })),
                        );
                        // The core refuses to go back inside a turn, so the button waits.
                        if let (Some(sequence), false) = (*sequence, chat.working) {
                            let (thread_id, text) = (shown.clone(), text.clone());
                            tools = tools.child(
                                Button::new(SharedString::from(format!("rollback-{key}")))
                                    .ghost()
                                    .xsmall()
                                    .icon(IconName::Undo2)
                                    .tooltip("Go back to this message")
                                    .on_click(cx.listener(move |ws, _, _, cx| {
                                        ws.ask_rollback(&thread_id, sequence, at, &text, cx)
                                    })),
                            );
                        }
                        div()
                            .id(group.clone())
                            .group(group)
                            .self_end()
                            .max_w(px(600.))
                            .flex()
                            .flex_col()
                            .items_end()
                            .gap_1()
                            .child(
                                div()
                                    .px_4()
                                    .py_2()
                                    .rounded_xl()
                                    .bg(surface)
                                    // Selectable like the answers; pastes and files show as labels.
                                    .child(links::markdown(
                                        SharedString::from(format!("user-{key}")),
                                        user_text(text),
                                    )),
                            )
                            .child(tools)
                            .into_any_element()
                    }
                    Item::Command(run) => command_card(&key, run, cx),
                    Item::Agent { from, text } => {
                        let unfolded = self.unfolded.contains(&(shown.clone(), at));
                        let id = shown.clone();
                        let first = text.lines().next().unwrap_or_default();
                        let mark = if unfolded { "\u{25BE}" } else { "\u{25B8}" };
                        let mut card = div().flex().flex_col().gap_2().child(
                            div()
                                .id(SharedString::from(format!("agent-{key}")))
                                .meta()
                                .text_color(muted)
                                .cursor_pointer()
                                .truncate()
                                .child(if unfolded {
                                    format!("{mark} {from} reported")
                                } else {
                                    format!("{mark} {from} reported: {first}")
                                })
                                .on_click(cx.listener(move |ws, _, _, cx| ws.fold(&id, at, cx))),
                        );
                        if unfolded {
                            card =
                                card.child(div().pl_3().border_l_2().border_color(border).child(
                                    links::markdown(
                                        SharedString::from(format!("report-{key}")),
                                        text.clone(),
                                    ),
                                ));
                        }
                        card.into_any_element()
                    }
                    Item::Assistant { text, reasoning } => {
                        let unfolded = self.unfolded.contains(&(shown.clone(), at));
                        let mut answer = div().flex().flex_col().gap_2();
                        if !reasoning.trim().is_empty() {
                            let id = shown.clone();
                            answer = answer.child(
                                div()
                                    .id(SharedString::from(format!("reasoning-{key}")))
                                    .small()
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
                                        .meta()
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
                            let (copied, spoken) = (text.clone(), text.clone());
                            let busy = self.synthesizing;
                            answer = answer.child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .gap_1()
                                    .child(
                                        Button::new(SharedString::from(format!("copy-{key}")))
                                            .ghost()
                                            .xsmall()
                                            .icon(IconName::Copy)
                                            .tooltip("Copy the answer")
                                            .on_click(cx.listener(move |_, _, _, cx| {
                                                cx.write_to_clipboard(ClipboardItem::new_string(
                                                    copied.clone(),
                                                ))
                                            })),
                                    )
                                    .child(
                                        Button::new(SharedString::from(format!("listen-{key}")))
                                            .ghost()
                                            .xsmall()
                                            .icon(IconName::Play)
                                            .label(if busy { "Preparing..." } else { "Listen" })
                                            .disabled(busy)
                                            .tooltip("Read the answer aloud")
                                            .on_click(cx.listener(move |ws, _, _, cx| {
                                                ws.speak_answer(&spoken, cx)
                                            })),
                                    ),
                            );
                        }
                        answer.into_any_element()
                    }
                    Item::Image(image) => picture(&self.status.artifacts_dir, image, cx),
                    Item::Audio(clip) => super::audio::card(self, &key, clip, cx),
                    Item::Notice(text) => div()
                        .meta()
                        .text_color(muted)
                        .child(text.clone())
                        .into_any_element(),
                    Item::Error(text) => div()
                        .px_3()
                        .py_2()
                        .rounded_md()
                        .meta()
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

/// A picture of the pictures folder. Only an ID of hex digits and dashes is taken, so
/// it cannot name a path outside that folder.
pub fn picture_path(folder: &Path, id: &str) -> Option<std::path::PathBuf> {
    let valid = !id.is_empty() && id.chars().all(|c| c.is_ascii_hexdigit() || c == '-');
    valid.then(|| folder.join(id).join("image"))
}

/// A sent message as the user wrote it, each paste and kept file as a short label.
fn user_text(text: &str) -> String {
    use dusk_protocol::pasted::{Part, split};
    split(text)
        .into_iter()
        .map(|part| match part {
            Part::Text(text) => text.to_owned(),
            Part::Pasted(pasted) => format!("`[{}]`", super::composer::paste_label(pasted)),
            Part::Source(name) => format!("`[{name}]`"),
        })
        .collect()
}

/// A message as one short line for a list: attachments by name, and an answer to the
/// agent's question as the answer alone.
pub fn label(text: &str) -> String {
    use dusk_protocol::pasted::{Part, split};
    let text = match text
        .strip_prefix("QUESTION: ")
        .and_then(|rest| rest.split_once("\nANSWER: "))
    {
        Some((_, answer)) => format!("Answer: {answer}"),
        None => text.to_owned(),
    };
    let plain: String = split(&text)
        .into_iter()
        .map(|part| match part {
            Part::Text(text) => text.to_owned(),
            Part::Pasted(pasted) => format!("[{}]", super::composer::paste_label(pasted)),
            Part::Source(name) => format!("[{name}]"),
        })
        .collect();
    plain.split_whitespace().collect::<Vec<_>>().join(" ")
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
    let (muted, border) = (cx.theme().muted_foreground, cx.theme().border);
    let open = ws.unfolded.contains(&(shown.to_owned(), start | RUN));
    // The steps of a run sit on a rail: they are a sequence, and read apart from the answer.
    let mut block = div()
        .flex()
        .flex_col()
        .gap_1()
        .pl_3()
        .border_l_1()
        .border_color(border);
    if tools.len() > 1 {
        let id = shown.to_owned();
        block = block.child(
            div()
                .id(SharedString::from(format!("run-{shown}-{start}")))
                .small()
                .text_color(muted)
                .cursor_pointer()
                .child(if open {
                    format!("▾ {}", crate::widgets::count(tools.len(), "step"))
                } else {
                    format!(
                        "▸ {} earlier",
                        crate::widgets::count(tools.len() - 1, "step")
                    )
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
    let (muted, success, danger, signal, surface) = (
        theme.muted_foreground,
        theme.success,
        theme.danger,
        theme.warning,
        theme.muted,
    );
    let open = ws.unfolded.contains(&(shown.to_owned(), index | STEP));
    let (icon, color) = match tool.state {
        // The signal color is for work going on; what is done recedes.
        ToolState::Running => (IconName::LoaderCircle, signal),
        ToolState::Done => (IconName::Check, success.opacity(0.7)),
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
            .small()
            .text_color(muted)
            .cursor_pointer()
            .child(widgets::icon(icon, color))
            .child(
                div()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme.foreground.opacity(0.85))
                    .child(tool.name.clone()),
            )
            .child(
                div()
                    .truncate()
                    .font_family(mono.clone())
                    .child(tool.detail.clone()),
            )
            .on_click(cx.listener(move |ws, _, _, cx| ws.fold(&id, index | STEP, cx)))
            // A long command can go on in the background while the agent works on.
            .when(
                tool.state == ToolState::Running && tool.name == "run_command",
                |row| {
                    let (thread_id, call_id) = (shown.to_owned(), tool.call_id.clone());
                    row.child(
                        Button::new(SharedString::from(format!("background-{shown}-{index}")))
                            .ghost()
                            .xsmall()
                            .label("To background")
                            .tooltip("The agent goes on; the result comes when it ends")
                            .on_click(cx.listener(move |ws, _, _, cx| {
                                cx.stop_propagation();
                                ws.send_to_background(&thread_id, &call_id, cx);
                            })),
                    )
                },
            ),
    );
    if open {
        let block = |title: &'static str, text: &str| {
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(div().meta().text_color(muted).child(title))
                .child(
                    div()
                        .p_2()
                        .rounded_md()
                        .bg(surface)
                        .meta()
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

/// A command the user ran from an answer: its line, a bar that fills while it runs, then
/// its output as a code block. A command has no known length, so the bar closes on the
/// end like a kettle: fast at first, slower after, full only when the command is done.
fn command_card(key: &str, run: &CommandRun, cx: &mut Context<Workspace>) -> AnyElement {
    let theme = cx.theme();
    let (muted, accent, danger, track) = (
        theme.muted_foreground,
        theme.primary,
        theme.danger,
        theme.border,
    );
    let mut card = div().flex().flex_col().gap_2().child(
        div()
            .meta()
            .text_color(muted)
            .child(format!("$ {}", run.command)),
    );
    match &run.result {
        None => {
            let started = run.started;
            card = card.child(
                div()
                    .h(px(3.))
                    .w_full()
                    .rounded_full()
                    .bg(track)
                    .overflow_hidden()
                    .child(
                        div()
                            .h_full()
                            .rounded_full()
                            .bg(accent)
                            .w(relative(0.02))
                            // The animation only drives the redraws; the width follows the clock.
                            .with_animation(
                                SharedString::from(format!("run-bar-{key}")),
                                Animation::new(std::time::Duration::from_millis(250)).repeat(),
                                move |bar, _| bar.w(relative(run_fraction(started.elapsed()))),
                            ),
                    ),
            );
        }
        Some(Ok(output)) => {
            let fenced = format!("```\n{}\n```", output.trim_end());
            card = card.child(links::markdown(
                SharedString::from(format!("command-{key}")),
                fenced,
            ));
        }
        Some(Err(error)) => {
            card = card.child(div().meta().text_color(danger).child(error.clone()));
        }
    }
    card.into_any_element()
}

/// How full the bar is after `elapsed`: `1 - e^(-t/6s)`, held below the end.
fn run_fraction(elapsed: std::time::Duration) -> f32 {
    (1. - (-elapsed.as_secs_f32() / 6.).exp()).clamp(0.02, 0.97)
}

/// A picture from the pictures folder as a card: the picture on the left, its title and
/// description beside it, a hairline under it to part it from the next. Only an ID of
/// hex digits and dashes is taken, so it cannot name a path outside that folder.
fn picture(folder: &Path, image: &Picture, cx: &mut Context<Workspace>) -> AnyElement {
    let id = &image.id;
    let Some(path) = picture_path(folder, id) else {
        return div().into_any_element();
    };
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
        text = text.child(div().meta().text_color(muted).child("Picture"));
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
                .id(SharedString::from(format!("picture-card-{id}")))
                .w(px(220.))
                .flex_none()
                .rounded_xl()
                .overflow_hidden()
                .bg(surface)
                .cursor_pointer()
                .child(
                    img(path.clone())
                        .w_full()
                        .max_h(px(320.))
                        .object_fit(ObjectFit::Cover),
                )
                .on_click(cx.listener(move |ws, _, _, cx| {
                    ws.modal = Some(crate::modal::Modal::Picture { path: path.clone() });
                    cx.notify();
                })),
        )
        .child(text)
        .into_any_element()
}

impl Workspace {
    fn ask_rollback(
        &mut self,
        thread_id: &str,
        sequence: u64,
        at: usize,
        text: &str,
        cx: &mut Context<Self>,
    ) {
        self.modal = Some(crate::modal::Modal::Rollback {
            thread_id: thread_id.to_owned(),
            sequence,
            at,
            text: text.to_owned(),
        });
        cx.notify();
    }

    /// Takes the chat back to before the message: the core forgets it and all after it,
    /// and the view is cut when the core says so. The message returns to the box.
    pub fn rollback(&mut self, cx: &mut Context<Self>) {
        let Some(crate::modal::Modal::Rollback {
            thread_id,
            sequence,
            at,
            text,
        }) = self.modal.take()
        else {
            return;
        };
        if let Some(chat) = self.chats.get_mut(&thread_id) {
            chat.rollback_at = Some(at);
        }
        let id = thread_id.clone();
        self.core.call(
            cx,
            move |client| async move {
                client
                    .threads()
                    .send(&thread_id, format!("/undo {sequence}"), false)
                    .await
            },
            move |ws, result, cx| match result {
                Ok(()) => ws.fill_composer = Some(text),
                Err(error) => {
                    if let Some(chat) = ws.chats.get_mut(&id) {
                        chat.rollback_at = None;
                    }
                    ws.fail(error, cx);
                }
            },
        );
        cx.notify();
    }

    fn send_to_background(&mut self, thread_id: &str, call_id: &str, cx: &mut Context<Self>) {
        let (thread_id, call_id) = (thread_id.to_owned(), call_id.to_owned());
        self.core.call(
            cx,
            move |client| async move {
                client
                    .threads()
                    .send_to_background(&thread_id, &call_id)
                    .await
            },
            |ws, result, cx| match result {
                Ok(()) => ws.refresh_inspector(cx),
                Err(error) => ws.fail(error, cx),
            },
        );
    }

    fn fold(&mut self, chat: &str, key: usize, cx: &mut Context<Self>) {
        let key = (chat.to_owned(), key);
        if !self.unfolded.remove(&key) {
            self.unfolded.insert(key);
        }
        self.measure_again = true;
        cx.notify();
    }
}
