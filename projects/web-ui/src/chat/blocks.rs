//! The transcript: the items of a chat, with runs of tool steps folded into one line.

use super::data::{ChatData, Entry, Item, Shown, Tool, ToolState};
use crate::{
    markdown::Markdown,
    state::{Chat, state},
    voice,
};
use dusk_protocol::thread::{Part, split};
use leptos::{html, prelude::*};
use wasm_bindgen::JsCast;

/// What the list draws: one item, or what the agent did on its way to an answer, folded
/// into one line that says how long it worked.
#[derive(Clone, PartialEq)]
enum Block {
    One(Entry),
    Turn {
        entries: Vec<Entry>,
        /// From the user's message to the answer; `None` while the turn goes on.
        took: Option<i64>,
    },
}

/// What tells one drawn block from another: whether it is a turn's fold, and the first
/// item in it. The fold of a turn may start with its answer's own thinking, which has
/// the answer's number, so the number alone would name two blocks.
type BlockKey = (bool, u64);

impl Block {
    fn key(&self) -> BlockKey {
        match self {
            Block::One(entry) => (false, entry.id),
            Block::Turn { entries, .. } => (true, entries.first().map_or(0, |entry| entry.id)),
        }
    }
}

/// What a finished turn folds away: all but its answer, the user's words and errors.
fn folds(item: &Item) -> bool {
    !matches!(item, Item::User(_) | Item::Error(_))
}

/// The chat as blocks: each turn's way to its answer in one, the rest one by one.
fn blocks(chat: &ChatData) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut at = 0;
    while at < chat.items.len() {
        let asked = &chat.items[at];
        blocks.push(Block::One(asked.clone()));
        let end = chat.items[at + 1..]
            .iter()
            .position(|entry| matches!(entry.item, Item::User(_)))
            .map_or(chat.items.len(), |offset| at + 1 + offset);
        let turn = &chat.items[at + 1..end];
        let open = end == chat.items.len() && chat.working;
        // The answer is the last thing said after the steps; while the turn goes on,
        // everything is on the way.
        let answer = (!open)
            .then(|| {
                turn.iter().rposition(
                    |e| matches!(&e.item, Item::Assistant { text, .. } if !text.trim().is_empty()),
                )
            })
            .flatten();
        // While the turn goes on, what the agent says stays in sight and only its steps
        // fold; once it is over, all but the answer folds.
        let folds_now = |item: &Item| {
            if open {
                matches!(item, Item::Tool(_) | Item::Agent { .. })
                    || matches!(item, Item::Assistant { text, .. } if text.trim().is_empty())
            } else {
                folds(item)
            }
        };
        let mut folded: Vec<Entry> = turn
            .iter()
            .enumerate()
            .filter(|(index, entry)| Some(*index) != answer && folds_now(&entry.item))
            .map(|(_, entry)| entry.clone())
            .collect();
        // The answer's own thinking folds with the rest; the answer shows only what it says.
        let answer_entry = answer.map(|index| {
            let mut entry = turn[index].clone();
            if let Item::Assistant { reasoning, .. } = &mut entry.item
                && !reasoning.trim().is_empty()
            {
                folded.push(Entry {
                    item: Item::Assistant {
                        text: String::new(),
                        reasoning: std::mem::take(reasoning),
                    },
                    ..turn[index].clone()
                });
            }
            entry
        });
        let started = matches!(asked.item, Item::User(_)).then_some(asked.at);
        let took = answer
            .zip(started)
            .map(|(index, started)| turn[index].at - started);
        if !folded.is_empty() {
            blocks.push(Block::Turn {
                entries: folded,
                took,
            });
        }
        for (index, entry) in turn.iter().enumerate() {
            if Some(index) == answer {
                blocks.extend(answer_entry.clone().map(Block::One));
            } else if !folds_now(&entry.item) {
                blocks.push(Block::One(entry.clone()));
            }
        }
        at = end;
    }
    blocks
}

use dusk_protocol::thread::duration_words as duration;

#[component]
pub fn Transcript(chat: Chat) -> impl IntoView {
    let scroller = NodeRef::<html::Div>::new();
    let all = Memo::new(move |_| chat.with(blocks));
    let firsts = Memo::new(move |_| all.with(|all| all.iter().map(Block::key).collect::<Vec<_>>()));
    // A second's tick for the time the running turn has taken.
    let tick = RwSignal::new(0u32);
    let _ = set_interval_with_handle(
        move || tick.update(|n| *n = n.wrapping_add(1)),
        std::time::Duration::from_secs(1),
    );
    let working_for = move || {
        tick.track();
        let started = chat.with(|c| {
            c.items
                .iter()
                .rev()
                .find(|entry| matches!(entry.item, Item::User(_)))
                .map(|entry| entry.at)
        });
        match started {
            Some(started) => format!(
                "Working \u{00B7} {}",
                duration(super::data::now_ms() - started)
            ),
            None => "Working".to_owned(),
        }
    };
    let pending = move || {
        chat.with(|c| c.pending.clone())
            .into_iter()
            .enumerate()
            .collect::<Vec<(usize, String)>>()
    };
    // Follow the end unless the reader scrolled up.
    Effect::new(move |_| {
        chat.with(|c| (c.items.len(), c.items.last().cloned(), c.working));
        let Some(div) = scroller.get_untracked() else {
            return;
        };
        let element: &web_sys::Element = div.unchecked_ref();
        let near_end =
            element.scroll_height() - element.scroll_top() - element.client_height() < 160;
        if near_end {
            request_animation_frame(move || element_to_end(&scroller));
        }
    });
    view! {
        <div class="transcript" node_ref=scroller>
            <Show when=move || chat.with(|c| c.items.is_empty() && c.pending.is_empty())>
                <div class="empty">"Ask anything. The chat is made with your first message."</div>
            </Show>
            <For each=move || firsts.get() key=|key| *key let:key>
                <BlockView all key />
            </For>
            <For each=pending key=|(at, text)| (*at, text.clone()) let:pair>
                <div class="msg user queued"><div class="bubble">{pair.1}</div><div class="muted">"queued"</div></div>
            </For>
            <Show when=move || chat.with(|c| c.working)>
                <div class="working"><span class="spinner"></span>{working_for}</div>
            </Show>
        </div>
    }
}

fn element_to_end(scroller: &NodeRef<html::Div>) {
    if let Some(div) = scroller.get_untracked() {
        let element: &web_sys::Element = div.unchecked_ref();
        element.set_scroll_top(element.scroll_height());
    }
}

#[component]
fn BlockView(all: Memo<Vec<Block>>, key: BlockKey) -> impl IntoView {
    let block = Memo::new(move |_| all.with(|all| all.iter().find(|b| b.key() == key).cloned()));
    move || match block.get() {
        Some(Block::One(entry)) => item_view(entry.item).into_any(),
        Some(Block::Turn { entries, took }) => view! { <Turn entries took /> }.into_any(),
        None => ().into_any(),
    }
}

fn item_view(item: Item) -> impl IntoView {
    match item {
        Item::User(text) => view! {
            <div class="msg user"><div class="bubble">{user_text(&text)}</div></div>
        }
        .into_any(),
        Item::Agent { from, text } => view! {
            <div class="msg agent">
                <div class="muted">{format!("From {from}")}</div>
                <Markdown text=text />
            </div>
        }
        .into_any(),
        Item::Assistant { text, reasoning } => assistant_view(text, reasoning).into_any(),
        Item::Tool(_) => ().into_any(),
        Item::Image(shown) => view! { <ImageCard shown /> }.into_any(),
        Item::Audio(shown) => view! { <AudioCard shown /> }.into_any(),
        Item::Notice(text) => view! { <div class="notice">{text}</div> }.into_any(),
        Item::Error(text) => view! { <div class="notice error">{text}</div> }.into_any(),
    }
}

/// A message as the user wrote it: each paste and kept file as a short label.
fn user_text(text: &str) -> String {
    split(text)
        .into_iter()
        .map(|part| match part {
            Part::Text(text) => text.to_owned(),
            Part::Pasted(pasted) => format!("[pasted {} lines]", pasted.lines().count().max(1)),
            Part::Source(name) => format!("[{name}]"),
        })
        .collect()
}

fn assistant_view(text: String, reasoning: String) -> impl IntoView {
    let has_reasoning = !reasoning.is_empty();
    let speak_text = text.clone();
    let copy_text = text.clone();
    view! {
        <div class="msg assistant">
            {has_reasoning.then(|| view! {
                <details class="reasoning">
                    <summary>"Thinking"</summary>
                    <Markdown text=reasoning />
                </details>
            })}
            <Markdown text=text />
            <div class="msg-actions">
                <button class="icon" title="Copy" on:click=move |_| copy(&copy_text)>"Copy"</button>
                <button class="icon" title="Read aloud" on:click=move |_| voice::speak(speak_text.clone())>"Listen"</button>
            </div>
        </div>
    }
}

fn copy(text: &str) {
    // The clipboard API is still unstable in web-sys, so it is reached by name.
    let Some(window) = web_sys::window() else {
        return;
    };
    let clipboard = js_sys::Reflect::get(&window.navigator(), &"clipboard".into()).ok();
    if let Some(clipboard) = clipboard
        && let Ok(write) = js_sys::Reflect::get(&clipboard, &"writeText".into())
        && let Some(write) = write.dyn_ref::<js_sys::Function>()
    {
        let _ = write.call1(&clipboard, &text.into());
    }
}

/// A turn's way to its answer: one line that says how long it took, or what it does now,
/// and under it every step, thought and remark.
#[component]
fn Turn(entries: Vec<Entry>, took: Option<i64>) -> impl IntoView {
    let running = took.is_none();
    let failed = entries
        .iter()
        .filter(|e| matches!(&e.item, Item::Tool(t) if t.state == ToolState::Failed))
        .count();
    let title = match took {
        Some(ms) => format!("Worked for {}", duration(ms)),
        None => entries
            .iter()
            .rev()
            .find_map(|e| match &e.item {
                Item::Tool(tool) if !tool.detail.trim().is_empty() => Some(tool.detail.clone()),
                Item::Tool(tool) => Some(tool.name.clone()),
                _ => None,
            })
            .unwrap_or_else(|| "Thinking".to_owned()),
    };
    let names = summary_line(&entries);
    view! {
        <details class="steps">
            <summary>
                {running.then(|| view! { <span class="spinner"></span> })}
                <span>{title}</span>
                <span class="muted">{names}</span>
                {(failed > 0).then(|| view! { <span class="bad">{format!("{failed} failed")}</span> })}
            </summary>
            {entries.into_iter().map(|entry| match entry.item {
                Item::Tool(tool) => view! { <ToolStep tool /> }.into_any(),
                Item::Assistant { text, reasoning } => view! {
                    <div class="step-said">
                        {(!reasoning.trim().is_empty()).then(|| view! {
                            <details class="reasoning"><summary>"Thinking"</summary><Markdown text=reasoning /></details>
                        })}
                        {(!text.trim().is_empty()).then(|| view! { <Markdown text=text /> })}
                    </div>
                }.into_any(),
                Item::Agent { from, text } => view! {
                    <div class="step-said"><div class="muted">{format!("From {from}")}</div><Markdown text=text /></div>
                }.into_any(),
                Item::Image(shown) => view! { <ImageCard shown /> }.into_any(),
                Item::Audio(shown) => view! { <AudioCard shown /> }.into_any(),
                Item::Notice(text) => view! { <div class="step-said muted">{text}</div> }.into_any(),
                _ => ().into_any(),
            }).collect_view()}
        </details>
    }
}

/// The tools of a run by name with counts, such as `bash x3, read x2`.
fn summary_line(steps: &[Entry]) -> String {
    let mut counts: Vec<(String, usize)> = Vec::new();
    for entry in steps {
        if let Item::Tool(tool) = &entry.item {
            match counts.iter_mut().find(|(name, _)| *name == tool.name) {
                Some((_, n)) => *n += 1,
                None => counts.push((tool.name.clone(), 1)),
            }
        }
    }
    counts
        .into_iter()
        .map(|(name, n)| {
            if n == 1 {
                name
            } else {
                format!("{name} ×{n}")
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

#[component]
fn ToolStep(tool: Tool) -> impl IntoView {
    let state_class = match tool.state {
        ToolState::Running => "running",
        ToolState::Done => "done",
        ToolState::Failed => "failed",
    };
    view! {
        <details class=format!("tool {state_class}")>
            <summary>
                <span class=if dusk_protocol::tools::is_agent_tool(&tool.name) { "tool-name agent" } else { "tool-name" }>
                    {dusk_protocol::tools::phrase(&tool.name, &tool.call_id)}
                </span>
                <span class="muted">{tool.detail.clone()}</span>
            </summary>
            <div class="muted mono">{tool.name.clone()}</div>
            <pre class="tool-args">{tool.arguments.clone()}</pre>
            {(!tool.output.is_empty()).then(|| view! { <pre class="tool-out">{tool.output.clone()}</pre> })}
        </details>
    }
}

#[component]
fn ImageCard(shown: Shown) -> impl IntoView {
    let app = state();
    let _ = app;
    let alt = shown
        .title
        .clone()
        .or(shown.description.clone())
        .unwrap_or_else(|| "picture".to_owned());
    view! {
        <figure class="card">
            <img src=format!("/api/card/{}/image", shown.id) alt=alt.clone() loading="lazy" />
            {shown.description.map(|d| view! { <figcaption class="muted">{d}</figcaption> })}
        </figure>
    }
}

#[component]
fn AudioCard(shown: Shown) -> impl IntoView {
    view! {
        <div class="card">
            <div class="muted">{shown.title.clone().unwrap_or_else(|| "Sound".to_owned())}</div>
            <audio controls src=format!("/api/card/{}/audio", shown.id)></audio>
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dusk_protocol::{model::ModelEvent, thread::ThreadEvent};

    /// A turn of thinking and an answer, with no tool: the fold holds only the answer's
    /// thinking. Two blocks under one key made the list draw the fold twice and never
    /// the answer.
    #[test]
    fn a_turn_of_thought_and_answer_draws_each_block_once() {
        let mut chat = ChatData::default();
        // Any item opens the turn the way the user's message does.
        chat.apply(ThreadEvent::Rejected {
            reason: "first".into(),
        });
        chat.apply(ThreadEvent::Model(ModelEvent::Reasoning("hm".into())));
        chat.apply(ThreadEvent::Model(ModelEvent::Text("done".into())));
        let blocks = blocks(&chat);
        assert_eq!(blocks.len(), 3);
        assert!(matches!(&blocks[1], Block::Turn { .. }));
        assert!(matches!(&blocks[2], Block::One(entry)
            if matches!(&entry.item, Item::Assistant { text, .. } if text == "done")));
        assert_ne!(blocks[1].key(), blocks[2].key());
    }
}
