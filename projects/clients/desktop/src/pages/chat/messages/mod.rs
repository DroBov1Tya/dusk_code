//! The transcript drawn as a virtual list: only the rows in sight are laid out, their
//! heights cached, so a long chat scrolls without lag. Messages are markdown, reasoning
//! folds, pictures are cards, and the agent's steps fold so only the latest shows. The
//! list follows new output while it is scrolled to the end. `row` draws one row, `activity`
//! a group of steps folded into one, `step` the agent's steps, and `command`, `picture` and `cost` the
//! cards and notes beside them; `sent_picture` the pictures a user's message carries.

mod activity;
mod command;
mod cost;
mod picture;
mod row;
mod sent_picture;
mod step;

pub use activity::{activity_text, agent_report, notice};
pub use picture::picture_path;
pub use row::label;
pub use sent_picture::SentPicture;
pub use step::{run, steps};

use gpui_kit::*;

use super::transcript::{Item, Transcript};
use crate::{ui::widgets, workspace::Workspace};

/// Keys of unfolded things share one set: answers by index, runs and steps apart.
pub(crate) const RUN: usize = 1 << 40;
pub(crate) const STEP: usize = 1 << 41;
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
        /// What it spent, as tokens and micro-dollars, when that was seen.
        spent: Option<(u64, u64)>,
    },
    Working,
}

pub fn render(ws: &mut Workspace, _: &mut Window, cx: &mut Context<Workspace>) -> AnyElement {
    let shown = ws.shown.clone().unwrap_or_default();
    let Some(chat) = ws.chats.get(&shown) else {
        return widgets::muted("Opening...", cx).into_any_element();
    };
    let rows = transcript_rows(chat);
    let sent = rows
        .iter()
        .enumerate()
        .filter_map(|(row, entry)| match *entry {
            Row::Item(index) => match &chat.items[index] {
                Item::User {
                    text,
                    sequence: Some(_),
                } => Some((row, text.clone())),
                _ => None,
            },
            _ => None,
        })
        .collect();
    // A chat opened from a found message shows that message, not its end.
    let jump = ws
        .jump
        .as_ref()
        .filter(|(chat, _)| *chat == shown)
        .map(|(_, words)| {
            let words = words.to_lowercase();
            let says = |item: &Item| match item {
                Item::User { text, .. } | Item::Assistant { text, .. } => {
                    text.to_lowercase().contains(&words)
                }
                _ => false,
            };
            rows.iter().position(|row| match *row {
                Row::Item(index) => says(&chat.items[index]),
                Row::Activity { .. } | Row::Working => false,
            })
        });
    sync(ws, &shown, &rows);
    if let Some(at) = jump {
        ws.jump = None;
        if let Some(item_ix) = at {
            ws.list.scroll_to(ListOffset {
                item_ix,
                offset_in_item: px(0.),
            });
        }
    }
    ws.rows = rows;
    ws.sent_rows = sent;
    list(
        ws.list.clone(),
        cx.processor(|ws, ix: usize, _window, cx| ws.transcript_row(ix, cx)),
    )
    .size_full()
    .into_any_element()
}

fn transcript_rows(chat: &Transcript) -> Vec<Row> {
    let mut rows = Vec::new();
    let mut start = 0;
    for (turn, end) in chat
        .activity_boundaries
        .iter()
        .copied()
        .chain(std::iter::once(chat.items.len()))
        .enumerate()
    {
        let first_row = rows.len();
        let mut group = start;
        for index in start..end {
            let item = &chat.items[index];
            let visible = match item {
                Item::Assistant { text, .. } => !text.trim().is_empty(),
                Item::User { sequence, .. } => sequence.is_some(),
                Item::Tool(_) | Item::Agent { .. } => false,
                _ => true,
            };
            if visible {
                // Reasoning was recorded together with the public text, but it belongs
                // above it. Only the reasoning is drawn in the activity disclosure.
                let through = index + usize::from(primary_activity(item));
                add_activity(&mut rows, &chat.items, group, through);
                rows.push(Row::Item(index));
                group = index + 1;
            }
        }
        add_activity(&mut rows, &chat.items, group, end);
        // Elapsed time and spend describe the whole turn. Show them once.
        if let Some(Row::Activity { span, spent, .. }) = rows[first_row..]
            .iter_mut()
            .rfind(|row| matches!(row, Row::Activity { .. }))
        {
            *span = chat.span(turn);
            *spent = chat.turn_spend(turn);
        }
        start = end;
    }
    if chat.working {
        rows.push(Row::Working);
    }
    rows
}

fn add_activity(rows: &mut Vec<Row>, items: &[Item], start: usize, end: usize) {
    let group = &items[start..end];
    if group.iter().any(primary_activity) {
        rows.push(Row::Activity {
            start,
            end,
            span: None,
            spent: None,
        });
    } else {
        for (offset, item) in group.iter().enumerate() {
            if !matches!(item, Item::Assistant { .. }) {
                rows.push(Row::Item(start + offset));
            }
        }
    }
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
    // Measuring a row again sends a reader inside it back to its top, which a long last
    // answer shows as a chat that cannot be scrolled down: remember where they are.
    let reading = (!ws.list.is_following_tail()).then(|| ws.list.logical_scroll_top());
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
    // As activity groups grow, keep the reader anchored to the same content.
    if let Some(top) = reading
        && let Some(was) = ws.rows.get(top.item_ix)
        && let Some(item_ix) = find(rows, was)
    {
        let same_kind = std::mem::discriminant(was) == std::mem::discriminant(&rows[item_ix]);
        ws.list.scroll_to(ListOffset {
            item_ix,
            offset_in_item: if same_kind {
                top.offset_in_item
            } else {
                px(0.)
            },
        });
    }
}

/// Where `rows` now shows what `was` showed: the same item, else the fold that took it in.
fn find(rows: &[Row], was: &Row) -> Option<usize> {
    let item = match *was {
        Row::Item(index) | Row::Activity { start: index, .. } => index,
        Row::Working => return rows.iter().position(|row| *row == Row::Working),
    };
    let exact = |row: &Row| match (*was, *row) {
        (Row::Item(_), Row::Item(index)) => index == item,
        (Row::Activity { .. }, Row::Activity { start, .. }) => start == item,
        _ => false,
    };
    rows.iter().position(exact).or_else(|| {
        rows.iter().position(
            |row| matches!(*row, Row::Activity { start, end, .. } if (start..end).contains(&item)),
        )
    })
}

fn primary_activity(item: &Item) -> bool {
    match item {
        Item::Tool(_) => true,
        Item::Assistant { reasoning, .. } => !reasoning.trim().is_empty(),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use dusk_protocol::{
        model::{ModelEvent, ToolCall},
        thread::{Message, MessageRole, ThreadEvent},
        tools::ToolResult,
    };

    use super::{Item, Row, Transcript, find, transcript_rows};

    fn call(id: &str) -> ToolCall {
        ToolCall {
            id: id.into(),
            name: "read_file".into(),
            arguments: serde_json::json!({ "path": id }),
        }
    }

    fn layout(chat: &Transcript) -> Vec<(usize, usize)> {
        transcript_rows(chat)
            .iter()
            .filter_map(|row| match *row {
                Row::Activity { start, end, .. } => Some((start, end)),
                Row::Item(index) => Some((index, index)),
                Row::Working => None,
            })
            .collect()
    }

    #[test]
    fn public_text_stays_between_activity_groups_as_the_turn_streams() {
        let mut chat = Transcript::default();
        chat.working = true;
        chat.apply(ThreadEvent::ToolStarted(call("a")));
        chat.apply(ThreadEvent::Model(ModelEvent::Reasoning("think".into())));
        chat.apply(ThreadEvent::Model(ModelEvent::Text("intermediate".into())));
        assert_eq!(layout(&chat), [(0, 2), (1, 1)]);

        chat.apply(ThreadEvent::ToolStarted(call("b")));
        assert_eq!(layout(&chat), [(0, 2), (1, 1), (2, 3)]);

        chat.apply(ThreadEvent::Model(ModelEvent::Text("final".into())));
        chat.apply(ThreadEvent::Idle);
        assert_eq!(layout(&chat), [(0, 2), (1, 1), (2, 3), (3, 3)]);
    }

    #[test]
    fn a_retry_notice_stays_visible_after_a_tool_finishes() {
        let mut chat = Transcript::default();
        chat.working = true;
        chat.apply(ThreadEvent::ToolStarted(call("a")));
        chat.apply(ThreadEvent::ToolFinished {
            call_id: "a".into(),
            result: ToolResult::text("done".into()),
        });
        chat.apply(ThreadEvent::Retrying {
            attempt: 2,
            error: "timeout".into(),
        });

        assert_eq!(layout(&chat), [(0, 1), (1, 1)]);
        assert!(matches!(&chat.items[1], Item::Notice(text) if text == "Retrying (2): timeout"));

        chat.apply(ThreadEvent::ToolStarted(call("b")));
        assert_eq!(layout(&chat), [(0, 1), (1, 1), (2, 3)]);
    }

    #[test]
    fn reopened_turn_keeps_public_text_outside_disclosures() {
        let message = |sequence: u64, content: &str, reasoning: Option<&str>, tool_calls| Message {
            message_id: sequence.to_string(),
            thread_id: "thread".into(),
            sequence,
            role: MessageRole::Assistant,
            content: content.into(),
            reasoning: reasoning.map(str::to_owned),
            tool_calls,
            tool_call_id: None,
            artifacts: Vec::new(),
            from_thread_id: None,
            from_job_id: None,
            content_tokens: 0,
            tool_calls_tokens: 0,
            reasoning_tokens: 0,
            created_at: 0,
        };
        let chat = Transcript::from_history(&[
            message(1_u64, "", None, vec![call("a")]),
            message(2, "intermediate", Some("think"), vec![call("b")]),
            message(3, "final", None, Vec::new()),
        ]);
        assert_eq!(layout(&chat), [(0, 2), (1, 1), (2, 3), (3, 3)]);
    }

    #[test]
    fn scroll_anchor_prefers_public_text_to_its_reasoning() {
        let rows = [
            Row::Activity {
                start: 0,
                end: 1,
                span: None,
                spent: None,
            },
            Row::Item(0),
        ];
        assert_eq!(find(&rows, &Row::Item(0)), Some(1));
    }
}
