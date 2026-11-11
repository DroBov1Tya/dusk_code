//! The transcript: the items of a chat, with runs of tool steps folded into one line.

use super::data::{Agent, ChatData, Entry, Item, Shown, Tool, ToolState};
use crate::{markdown::Markdown, state::{Chat, state}, voice};
use dusk_protocol::thread::{Part, split};
use leptos::{html, prelude::*};
use wasm_bindgen::JsCast;

/// What the list draws: one item, or a run of tool steps the user can unfold.
#[derive(Clone, PartialEq)]
enum Block {
    One(Entry),
    Steps(Vec<Entry>),
}

/// The block that starts at the item numbered `first`.
fn block_at(chat: &ChatData, first: u64) -> Option<Block> {
    let at = chat.items.binary_search_by_key(&first, |entry| entry.id).ok()?;
    let entry = &chat.items[at];
    if !matches!(entry.item, Item::Tool(_)) {
        return Some(Block::One(entry.clone()));
    }
    let steps = chat.items[at..]
        .iter()
        .take_while(|entry| matches!(entry.item, Item::Tool(_)))
        .cloned()
        .collect();
    Some(Block::Steps(steps))
}

/// The numbers of the first items of the blocks, in order.
fn starts(chat: &ChatData) -> Vec<u64> {
    let mut starts = Vec::new();
    let mut in_steps = false;
    for entry in &chat.items {
        let tool = matches!(entry.item, Item::Tool(_));
        if !(tool && in_steps) {
            starts.push(entry.id);
        }
        in_steps = tool;
    }
    starts
}

#[component]
pub fn Transcript(chat: Chat) -> impl IntoView {
    let scroller = NodeRef::<html::Div>::new();
    let blocks = Memo::new(move |_| chat.with(starts));
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
        let near_end = element.scroll_height() - element.scroll_top() - element.client_height() < 160;
        if near_end {
            request_animation_frame(move || element_to_end(&scroller));
        }
    });
    view! {
        <div class="transcript" node_ref=scroller>
            <Show when=move || chat.with(|c| c.items.is_empty() && c.pending.is_empty())>
                <div class="empty">"Ask anything. The chat is made with your first message."</div>
            </Show>
            <For each=move || blocks.get() key=|first| *first let:first>
                <BlockView chat first />
            </For>
            <For each=pending key=|(at, text)| (*at, text.clone()) let:pair>
                <div class="msg user queued"><div class="bubble">{pair.1}</div><div class="muted">"queued"</div></div>
            </For>
            <Show when=move || chat.with(|c| c.working)>
                <div class="working"><span class="spinner"></span>"Working"</div>
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
fn BlockView(chat: Chat, first: u64) -> impl IntoView {
    let block = Memo::new(move |_| chat.with(|c| block_at(c, first)));
    move || match block.get() {
        Some(Block::One(entry)) => item_view(entry.item).into_any(),
        Some(Block::Steps(steps)) => view! { <Steps steps /> }.into_any(),
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

#[component]
fn Steps(steps: Vec<Entry>) -> impl IntoView {
    let running = steps.iter().any(|e| matches!(&e.item, Item::Tool(t) if t.state == ToolState::Running));
    let failed = steps.iter().filter(|e| matches!(&e.item, Item::Tool(t) if t.state == ToolState::Failed)).count();
    let names = summary_line(&steps);
    let count = steps.len();
    view! {
        <details class="steps" open=running>
            <summary>
                {running.then(|| view! { <span class="spinner"></span> })}
                <span>{format!("{count} step{}", if count == 1 { "" } else { "s" })}</span>
                <span class="muted">{names}</span>
                {(failed > 0).then(|| view! { <span class="bad">{format!("{failed} failed")}</span> })}
            </summary>
            {steps.into_iter().map(|entry| match entry.item {
                Item::Tool(tool) => view! { <ToolStep tool /> }.into_any(),
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
        .map(|(name, n)| if n == 1 { name } else { format!("{name} ×{n}") })
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
                <span class="tool-name">{tool.name.clone()}</span>
                <span class="muted">{tool.detail.clone()}</span>
            </summary>
            <pre class="tool-args">{tool.arguments.clone()}</pre>
            {(!tool.output.is_empty()).then(|| view! { <pre class="tool-out">{tool.output.clone()}</pre> })}
        </details>
    }
}

#[component]
fn ImageCard(shown: Shown) -> impl IntoView {
    let app = state();
    let _ = app;
    let alt = shown.title.clone().or(shown.description.clone()).unwrap_or_else(|| "picture".to_owned());
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

/// The agents tile and window use the same step list.
pub fn agent_label(agent: &Agent) -> String {
    format!("{} · {}", agent.name, agent.role)
}
