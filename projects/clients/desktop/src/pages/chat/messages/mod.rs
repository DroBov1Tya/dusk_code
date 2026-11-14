//! The transcript drawn as a virtual list: only the rows in sight are laid out, their
//! heights cached, so a long chat scrolls without lag. Messages are markdown, reasoning
//! folds, pictures are cards, and the agent's steps fold so only the latest shows. The
//! list follows new output while it is scrolled to the end. `row` draws one row, `activity`
//! a turn folded into one, `step` the agent's steps, and `command`, `picture` and `cost` the
//! cards and notes beside them.

mod activity;
mod command;
mod cost;
mod picture;
mod row;
mod step;

pub use activity::{activity_text, agent_report};
pub use picture::picture_path;
pub use row::label;
pub use step::{run, steps};

use gpui_kit::*;

use super::transcript::Item;
use crate::{ui::widgets, workspace::Workspace};

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
                    spent: chat.turn_spend(turn),
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
                Row::Activity { start, end, .. } => chat.items[start..end].iter().any(says),
                Row::Working => false,
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
    if let Some(top) = reading
        && ws.rows.get(top.item_ix) == rows.get(top.item_ix)
        && top.item_ix < rows.len()
    {
        ws.list.scroll_to(top);
    }
}

fn primary_activity(item: &Item) -> bool {
    match item {
        Item::Tool(_) => true,
        Item::Assistant { reasoning, .. } => !reasoning.trim().is_empty(),
        _ => false,
    }
}
