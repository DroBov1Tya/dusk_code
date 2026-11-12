//! The transcript drawn as a virtual list: only the rows in sight are laid out, their
//! heights cached, so a long chat scrolls without lag. Messages are markdown, reasoning
//! folds, pictures are cards, and the agent's steps fold so only the latest shows. The
//! list follows new output while it is scrolled to the end.

use std::path::Path;

use gpui_kit::{
    component::{
        ActiveTheme, Colorize, Disableable, IconName, Sizable,
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
use crate::{
    ui::{
        Type,
        widgets::{self, Focusable},
    },
    workspace::Workspace,
};

/// Keys of unfolded things share one set: answers by index, runs and steps apart.
const RUN: usize = 1 << 40;
const STEP: usize = 1 << 41;
const ACTIVITY: usize = 1 << 42;

/// One row of the list: a visible item, a turn disclosure, or the working figure.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Row {
    Item(usize),
    Activity {
        start: usize,
        end: usize,
        /// The turn's begin and end, for how long it took.
        span: Option<super::timing::Span>,
    },
    Working,
}

pub fn render(ws: &mut Workspace, _: &mut Window, cx: &mut Context<Workspace>) -> AnyElement {
    let shown = ws.shown.clone().unwrap_or_default();
    let Some(chat) = ws.chats.get(&shown) else {
        return widgets::muted("Opening...", cx).into_any_element();
    };
    let mut rows = Vec::new();
    let mut sent = Vec::new();
    let mut start = 0;
    for (turn, end) in chat
        .activity_boundaries
        .iter()
        .copied()
        .chain(std::iter::once(chat.items.len()))
        .enumerate()
    {
        let items = &chat.items[start..end];
        let open = end == chat.items.len() && chat.working;
        // Only the answer stays in sight: the last thing said once the turn is over.
        // Everything before it in the turn, steps, thoughts, remarks and pictures,
        // folds into one row; the user's own words and errors stand apart.
        let answer = (!open)
            .then(|| {
                items.iter().rposition(
                    |item| matches!(item, Item::Assistant { text, .. } if !text.trim().is_empty()),
                )
            })
            .flatten()
            .map(|offset| start + offset);
        // While the turn goes on, what the agent says stays in sight and only its steps
        // fold; once it is over, all but the answer folds.
        let folds = |index: usize, item: &Item| {
            if open {
                return matches!(
                    item,
                    Item::Tool(_) | Item::Agent { .. } | Item::User { sequence: None, .. }
                ) || matches!(
                    item,
                    Item::Assistant { text, reasoning }
                        if text.trim().is_empty() && !reasoning.trim().is_empty()
                );
            }
            Some(index) != answer
                && !matches!(
                    item,
                    Item::User {
                        sequence: Some(_),
                        ..
                    } | Item::Error(_)
                )
                && !matches!(item, Item::Assistant { text, reasoning } if text.is_empty() && reasoning.trim().is_empty())
        };
        let activity = if items.iter().any(primary_activity) {
            let first = (start..end).find(|&index| folds(index, &chat.items[index]));
            let last = (start..end)
                .rev()
                .find(|&index| folds(index, &chat.items[index]));
            first.zip(last)
        } else {
            None
        };
        for (offset, item) in items.iter().enumerate() {
            let index = start + offset;
            if let Some((first, last)) = activity
                && index == last
            {
                rows.push(Row::Activity {
                    start: first,
                    end: last + 1,
                    span: chat.span(turn),
                });
            }
            match item {
                Item::Tool(_) => continue,
                Item::Assistant { text, .. } if text.is_empty() => continue,
                _ if activity.is_some_and(|(first, last)| {
                    (first..=last).contains(&index) && folds(index, item)
                }) =>
                {
                    continue;
                }
                Item::User {
                    text,
                    sequence: Some(_),
                } => sent.push((rows.len(), text.clone())),
                _ => {}
            }
            rows.push(Row::Item(index));
        }
        start = end;
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
    let changed = ws.rows.iter().zip(rows).position(|(a, b)| a != b);
    let first = changed.unwrap_or(same).min(same.saturating_sub(1));
    if old != rows.len() || changed.is_some() || ws.measure_again {
        ws.list.splice(first..old, rows.len() - first);
    } else if let Some(last) = rows.len().checked_sub(1) {
        ws.list.splice(last..last + 1, 1);
    }
    if ws.measure_again {
        ws.measure_again = false;
        ws.list.splice(0..rows.len(), rows.len());
    }
}

fn visible_tool(tool: &Tool) -> bool {
    !matches!(tool.name.as_str(), "display_image" | "display_audio")
        || tool.state != ToolState::Done
}

fn primary_activity(item: &Item) -> bool {
    match item {
        Item::Tool(_) => true,
        Item::Assistant { reasoning, .. } => !reasoning.trim().is_empty(),
        _ => false,
    }
}

/// The steps a run shows: a picture card stands for the step that showed it.
pub fn steps(items: &[Item]) -> Vec<&Tool> {
    items
        .iter()
        .filter_map(|item| match item {
            Item::Tool(tool) if visible_tool(tool) => Some(tool),
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
        let (muted, accent, danger) = (theme.muted_foreground, theme.primary, theme.danger);
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
                .child(match chat.span(chat.activity_boundaries.len()) {
                    Some((began, None)) => format!(
                        "Working \u{00B7} {}",
                        super::timing::duration(super::timing::now_ms() - began)
                    ),
                    _ => "Working".to_owned(),
                })
                .into_any_element(),
            Row::Activity { start, end, span } => {
                turn_activity(self, &shown, start, &chat.items[start..end], span, cx)
                    .into_any_element()
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
                                .small()
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
                                    .small()
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
                                // The user's words carry a breath of the accent, so the
                                // two sides of the chat part at a glance without glare.
                                div()
                                    .px_4()
                                    .py_2()
                                    .rounded_xl()
                                    .bg(accent.opacity(0.09))
                                    .border_1()
                                    .border_color(accent.opacity(0.16))
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
                        agent_report(self, &shown, at, from, text, cx).into_any_element()
                    }
                    Item::Assistant { text, .. } => {
                        let group = SharedString::from(format!("answer-row-{key}"));
                        let mut answer = div()
                            .id(group.clone())
                            .group(group.clone())
                            .flex()
                            .flex_col()
                            .gap_1();
                        if !text.is_empty() {
                            answer = answer.child(super::media::render(
                                &format!("answer-{key}"),
                                text,
                                cx,
                            ));
                            let (copied, spoken) = (text.clone(), text.clone());
                            let listening = self.audio.listening.as_deref() == Some(key.as_str());
                            let (spoken_key, other_busy) =
                                (key.clone(), self.audio.listening.is_some() && !listening);
                            // The answer reads as plain text; its tools come up under the
                            // pointer, or while it is read aloud.
                            answer = answer.child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .gap_1()
                                    .when(!listening, |row| {
                                        row.opacity(0.)
                                            .group_hover(group.clone(), |row| row.opacity(1.))
                                    })
                                    .child(
                                        Button::new(SharedString::from(format!("copy-{key}")))
                                            .ghost()
                                            .small()
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
                                            .small()
                                            .icon(if listening {
                                                IconName::Square
                                            } else {
                                                IconName::Play
                                            })
                                            .label(if listening { "Stop" } else { "Listen" })
                                            .disabled(other_busy)
                                            .tooltip(if listening {
                                                "Stop reading"
                                            } else {
                                                "Read the answer aloud"
                                            })
                                            .on_click(cx.listener(move |ws, _, _, cx| {
                                                if listening {
                                                    ws.stop_listening(cx);
                                                } else {
                                                    ws.speak_answer(&spoken_key, &spoken, cx);
                                                }
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
        let activity = matches!(row, Row::Activity { .. });
        let gutter = if activity { 3. } else { 8. };
        let top = if ix == 0 { 24. } else { gutter };
        let bottom = if ix + 1 == self.rows.len() {
            24.
        } else {
            gutter
        };
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
    use dusk_protocol::thread::{Part, split};
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
    use dusk_protocol::thread::{Part, split};
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

fn turn_activity(
    ws: &Workspace,
    shown: &str,
    start: usize,
    items: &[Item],
    span: Option<super::timing::Span>,
    cx: &mut Context<Workspace>,
) -> Div {
    let open = ws.unfolded.contains(&(shown.to_owned(), start | ACTIVITY));
    let mut latest = None;
    let mut running = None;
    for (index, item) in items.iter().enumerate().rev() {
        if let Item::Tool(tool) = item {
            if latest.is_none() {
                latest = Some(tool);
            }
            if running.is_none() && tool.state == ToolState::Running && tool.name == "run_command" {
                running = Some((start + index, tool));
            }
        }
        if latest.is_some() && running.is_some() {
            break;
        }
    }
    // A finished turn says how long it took; one under way, what it does now.
    let title = match span {
        Some((began, Some(ended))) => {
            format!("Worked for {}", super::timing::duration(ended - began))
        }
        _ => latest
            .map_or("Thinking", |tool| {
                dusk_protocol::tools::phrase(&tool.name, &tool.call_id)
            })
            .to_owned(),
    };
    let id = shown.to_owned();
    let header = disclosure(
        SharedString::from(format!("activity-{shown}-{start}")),
        open,
        cx,
    )
    .flex_1()
    .aria_label(format!("Turn activity, {title}"))
    .child(disclosure_icon(open, step_muted(cx)))
    .child(
        div()
            .min_w_0()
            .truncate()
            .text_color(cx.theme().foreground)
            .child(title),
    )
    .on_click(cx.listener(move |ws, _, _, cx| ws.fold(&id, start | ACTIVITY, cx)));
    let mut row = div().flex().items_center().gap_2().child(header);
    if !open && let Some((index, tool)) = running {
        row = row.child(background_button(shown, index, tool, cx));
    }
    let mut block = div().flex().flex_col().gap_1().child(row);
    if open {
        let mut detail = div()
            .ml_4()
            .pl_2()
            .border_l_1()
            .border_color(cx.theme().border)
            .flex()
            .flex_col()
            .gap_1();
        for (offset, item) in items.iter().enumerate() {
            let index = start + offset;
            detail = match item {
                Item::Tool(tool) => detail.child(step(ws, shown, index, tool, cx)),
                Item::Assistant { text, reasoning } => {
                    let mut detail = detail;
                    if !reasoning.trim().is_empty() {
                        detail = detail.child(activity_text(
                            ws,
                            shown,
                            index,
                            "Thinking",
                            || reasoning.clone().into_any_element(),
                            cx,
                        ));
                    }
                    // A running turn shows what it says in the chat, not here.
                    let running = matches!(span, Some((_, None)) | None);
                    if !text.trim().is_empty() && !running {
                        detail = detail.child(div().px_2().small().child(super::links::markdown(
                            SharedString::from(format!("said-{shown}-{index}")),
                            text.clone(),
                        )));
                    }
                    detail
                }
                Item::Agent { from, text } => {
                    detail.child(agent_report(ws, shown, index, from, text, cx))
                }
                Item::Image(image) => detail.child(picture(&ws.status.artifacts_dir, image, cx)),
                Item::Notice(text) => detail.child(
                    div()
                        .px_2()
                        .small()
                        .text_color(step_muted(cx))
                        .child(text.clone()),
                ),
                Item::User {
                    text,
                    sequence: None,
                } => detail.child(activity_text(
                    ws,
                    shown,
                    index,
                    "Background task result",
                    || text.clone().into_any_element(),
                    cx,
                )),
                _ => detail,
            };
        }
        block = block.child(widgets::unfold(
            SharedString::from(format!("unfold-{shown}-{start}")),
            detail,
        ));
    }
    block
}

fn agent_report(
    ws: &Workspace,
    shown: &str,
    index: usize,
    from: &str,
    text: &str,
    cx: &mut Context<Workspace>,
) -> Div {
    let label = if ws.unfolded.contains(&(shown.to_owned(), index)) {
        format!("{from} reported")
    } else {
        let first = text.lines().next().unwrap_or_default();
        format!("{from} reported: {first}")
    };
    activity_text(
        ws,
        shown,
        index,
        &label,
        || {
            links::markdown(
                SharedString::from(format!("report-{shown}-{index}")),
                text.to_owned(),
            )
            .into_any_element()
        },
        cx,
    )
}

fn activity_text(
    ws: &Workspace,
    shown: &str,
    index: usize,
    title: &str,
    body: impl FnOnce() -> AnyElement,
    cx: &mut Context<Workspace>,
) -> Div {
    let open = ws.unfolded.contains(&(shown.to_owned(), index));
    let id = shown.to_owned();
    let muted = step_muted(cx);
    div()
        .flex()
        .flex_col()
        .gap_1()
        .child(
            disclosure(
                SharedString::from(format!("activity-text-{shown}-{index}")),
                open,
                cx,
            )
            .aria_label(title.to_owned())
            .child(disclosure_icon(open, muted))
            .child(div().min_w_0().truncate().child(title.to_owned()))
            .on_click(cx.listener(move |ws, _, _, cx| ws.fold(&id, index, cx))),
        )
        .when(open, |block| {
            block.child(
                div()
                    .ml_4()
                    .pl_3()
                    .border_l_1()
                    .border_color(cx.theme().border)
                    .small()
                    .text_color(muted)
                    .child(body()),
            )
        })
}

/// A run of steps: the latest one, and the count of the others to unfold.
pub fn run(
    ws: &Workspace,
    shown: &str,
    start: usize,
    tools: &[&Tool],
    cx: &mut Context<Workspace>,
) -> Div {
    let open = ws.unfolded.contains(&(shown.to_owned(), start | RUN));
    let mut block = div().flex().flex_col().gap_1();
    if tools.len() > 1 {
        let id = shown.to_owned();
        let label = if open {
            widgets::count(tools.len(), "step")
        } else {
            format!("{} earlier", widgets::count(tools.len() - 1, "step"))
        };
        block = block.child(
            disclosure(SharedString::from(format!("run-{shown}-{start}")), open, cx)
                .aria_label(label.clone())
                .child(disclosure_icon(open, step_muted(cx)))
                .child(label)
                .on_click(cx.listener(move |ws, _, _, cx| ws.fold(&id, start | RUN, cx))),
        );
    }
    let first = if open { 0 } else { tools.len() - 1 };
    for (offset, tool) in tools.iter().enumerate().skip(first) {
        block = block.child(step(ws, shown, start + offset, tool, cx));
    }
    block
}

fn step_muted(cx: &App) -> Hsla {
    let theme = cx.theme();
    theme.foreground.mix_oklab(theme.muted_foreground, 0.16)
}

fn disclosure(id: impl Into<ElementId>, open: bool, cx: &App) -> Stateful<Div> {
    let hover = cx.theme().list_hover;
    div()
        .id(id)
        .focusable_row(cx)
        .role(Role::Button)
        .aria_expanded(open)
        .flex()
        .items_center()
        .gap_2()
        .min_h(px(28.))
        .min_w_0()
        .px_2()
        .rounded_md()
        .small()
        .text_color(step_muted(cx))
        .cursor_pointer()
        .when(open, |row| row.bg(hover))
        .hover(move |row| row.bg(hover))
}

fn disclosure_icon(open: bool, color: Hsla) -> impl IntoElement {
    widgets::icon(
        if open {
            IconName::ChevronDown
        } else {
            IconName::ChevronRight
        },
        color,
    )
}

fn step(
    ws: &Workspace,
    shown: &str,
    index: usize,
    tool: &Tool,
    cx: &mut Context<Workspace>,
) -> Div {
    let theme = cx.theme();
    let muted = step_muted(cx);
    let surface = theme.muted;
    let open = ws.unfolded.contains(&(shown.to_owned(), index | STEP));
    let (icon, color, status) = match tool.state {
        ToolState::Running => (IconName::LoaderCircle, theme.warning, "Running"),
        ToolState::Done => (IconName::Check, muted, "Completed"),
        ToolState::Failed => (IconName::CircleX, theme.danger, "Failed"),
    };
    let id = shown.to_owned();
    let mono = crate::ui::theme::prefs(cx)
        .mono_font
        .clone()
        .unwrap_or_else(|| "Menlo".to_owned());
    let detail = tool.detail.trim().to_owned();
    // Said as a person would; the tool's own name shows once the step is open.
    let summary = dusk_protocol::tools::phrase(&tool.name, &tool.call_id);
    // Work handed to another agent is the turn's big move: it carries the accent.
    let lead = if dusk_protocol::tools::is_agent_tool(&tool.name) {
        theme.primary
    } else {
        theme.foreground
    };
    let button = disclosure(
        SharedString::from(format!("step-{shown}-{index}")),
        open,
        cx,
    )
    .flex_1()
    .aria_label(format!("{summary}, {}, {status}", tool.name))
    .child(widgets::icon(icon, color))
    .child(div().flex_none().text_color(lead).child(summary))
    .child(
        div()
            .flex_1()
            .min_w_0()
            .truncate()
            .meta()
            .text_color(muted)
            .child(detail),
    )
    .when(open, |row| {
        row.child(
            div()
                .max_w(relative(0.30))
                .truncate()
                .meta()
                .font_family(mono.clone())
                .child(tool.name.clone()),
        )
    })
    .when(tool.state == ToolState::Failed, |row| {
        row.child(div().meta().text_color(color).child(status))
    })
    .child(disclosure_icon(open, muted))
    .on_click(cx.listener(move |ws, _, _, cx| ws.fold(&id, index | STEP, cx)));
    let mut row = div().flex().items_center().gap_2().child(button);
    // A long command can go on in the background while the agent works on.
    if tool.state == ToolState::Running && tool.name == "run_command" {
        row = row.child(background_button(shown, index, tool, cx));
    }
    let mut line = div().flex().flex_col().child(row);
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
            .pl_4()
            .pt_1()
            .child(block("Arguments", &tool.arguments));
        if !tool.output.is_empty() {
            details = details.child(block("Answer", &tool.output));
        }
        line = line.child(widgets::unfold(
            SharedString::from(format!("unfold-step-{shown}-{index}")),
            details,
        ));
    }
    line
}

fn background_button(
    shown: &str,
    index: usize,
    tool: &Tool,
    cx: &mut Context<Workspace>,
) -> Button {
    let (thread_id, call_id) = (shown.to_owned(), tool.call_id.clone());
    Button::new(SharedString::from(format!("background-{shown}-{index}")))
        .ghost()
        .small()
        .label("To background")
        .tooltip("The agent goes on; the result comes when it ends")
        .on_click(cx.listener(move |ws, _, _, cx| {
            cx.stop_propagation();
            ws.send_to_background(&thread_id, &call_id, cx);
        }))
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
