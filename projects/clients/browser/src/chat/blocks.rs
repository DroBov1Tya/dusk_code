//! The transcript: the items of a chat, with runs of tool steps folded into one line.
//!
//! The list is drawn in pieces that each follow only what they show. Which blocks there
//! are follows the chat's shape, which a streamed piece of text does not change; each item
//! follows its own revision. So a token redraws its answer alone, and a fold the reader
//! opened stays open while the turn under it goes on.

use super::data::{ChatData, Item};
use super::{
    markdown::Markdown, shown::ShownBlock, skill_change::SkillChanges, voice, working::Working,
};
use crate::{
    state::{Chat, View, state},
    ui::{Icon, IconName},
};
use dusk_protocol::thread::{ChatLog, GoalVerdict, Tool, ToolState, labelled};
use dusk_protocol::tools::Card;
use leptos::{html, prelude::*};
use std::collections::HashMap;
use std::sync::LazyLock;
use wasm_bindgen::JsCast;

/// The chat a transcript draws: one of the page's chats, or the chat of a subagent of it.
#[derive(Clone, Copy)]
pub struct Source {
    chat: Chat,
    agent: Option<StoredValue<String>>,
}

/// What a subagent's chat is before its first event.
static EMPTY: LazyLock<ChatLog> = LazyLock::new(ChatLog::default);

impl Source {
    pub fn of(chat: Chat) -> Self {
        Self { chat, agent: None }
    }

    pub fn subagent(chat: Chat, thread_id: String) -> Self {
        Self {
            chat,
            agent: Some(StoredValue::new(thread_id)),
        }
    }

    pub fn chat(self) -> Chat {
        self.chat
    }

    fn pick<R>(self, data: &ChatData, f: impl FnOnce(&ChatLog) -> R) -> R {
        match self.agent {
            None => f(&data.log),
            Some(agent) => agent.with_value(|id| f(data.agent_chats.get(id).unwrap_or(&EMPTY))),
        }
    }

    /// Reads the chat, following its changes but not streamed text.
    pub fn with<R>(self, f: impl FnOnce(&ChatLog) -> R) -> R {
        self.chat.with(|data| {
            if self.agent.is_some() {
                data.subagents.track();
            }
            self.pick(data, f)
        })
    }

    pub fn with_untracked<R>(self, f: impl FnOnce(&ChatLog) -> R) -> R {
        self.chat.with_untracked(|data| self.pick(data, f))
    }

    /// Follows streamed text as well as every other change.
    fn track_all(self) {
        self.chat
            .with(|data| data.track_streams(self.agent.is_some()));
    }

    /// Changes the chat and tells every view of it.
    pub fn update(self, f: impl FnOnce(&mut ChatLog)) {
        self.chat.update(|data| match self.agent {
            None => f(&mut data.log),
            Some(agent) => {
                agent.with_value(|id| f(data.agent_chats.entry(id.clone()).or_default()))
            }
        });
    }
}

/// What of an item a block draws: all of it, an answer's words without its thinking, or
/// its thinking alone, folded with the steps of its turn.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Part {
    Whole,
    Said,
    Thought,
}

/// What the list draws: one item, or what the agent did on its way to an answer, folded
/// into one line that says how long it worked. Items by their ID.
#[derive(Clone, Debug, PartialEq)]
enum Block {
    One(u64, Part),
    Turn {
        /// The item the turn starts from, which names the fold.
        opener: u64,
        steps: Vec<(u64, Part)>,
        /// From the user's message to the answer; `None` while the turn goes on.
        took: Option<i64>,
    },
}

/// What tells one drawn block from another: whether it is a turn's fold, and its item, or
/// for a fold the item its turn starts from. What a fold holds changes as its turn goes on
/// and ends, but its name does not, so it is never drawn anew: an open fold stays open.
type BlockKey = (bool, u64);

impl Block {
    fn key(&self) -> BlockKey {
        match self {
            Block::One(id, _) => (false, *id),
            Block::Turn { opener, .. } => (true, *opener),
        }
    }
}

/// What opens a turn: the user's words or a job's result.
fn opens_turn(item: &Item) -> bool {
    matches!(item, Item::User { .. } | Item::Job(_))
}

/// What a finished turn folds away: all but its answer, the user's words, the blocks and
/// Workshop cards the agent showed, the runs it started, the goal's verdict on it, and
/// errors.
fn folds(item: &Item) -> bool {
    !matches!(
        item,
        Item::User { .. }
            | Item::Job(_)
            | Item::Block(_)
            | Item::Suggestion(_)
            | Item::Proposal { .. }
            | Item::Run(_)
            | Item::Goal(_)
            | Item::Error(_)
    )
}

/// The chat as blocks: each turn's way to its answer in one, the rest one by one.
fn blocks(chat: &ChatLog) -> Vec<Block> {
    let items = &chat.items;
    let mut blocks = Vec::new();
    let mut at = 0;
    while at < items.len() {
        blocks.push(Block::One(chat.id(at), Part::Whole));
        let end = items[at + 1..]
            .iter()
            .position(opens_turn)
            .map_or(items.len(), |offset| at + 1 + offset);
        let open = end == items.len() && chat.working;
        // The answer is the last thing said after the steps; while the turn goes on,
        // everything is on the way.
        let answer = (!open)
            .then(|| {
                (at + 1..end).rev().find(|&index| {
                    matches!(&items[index], Item::Assistant { text, .. } if !text.trim().is_empty())
                })
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
        let mut steps: Vec<(u64, Part)> = (at + 1..end)
            .filter(|&index| Some(index) != answer && folds_now(&items[index]))
            .map(|index| (chat.id(index), Part::Whole))
            .collect();
        // The answer's own thinking folds with the rest; the answer shows only what it says.
        let mut answer_part = Part::Whole;
        if let Some(index) = answer
            && let Item::Assistant { reasoning, .. } = &items[index]
            && !reasoning.trim().is_empty()
        {
            steps.push((chat.id(index), Part::Thought));
            answer_part = Part::Said;
        }
        let started = opens_turn(&items[at]).then(|| chat.at(at)).flatten();
        let took = answer
            .zip(started)
            .and_then(|(index, started)| Some(chat.at(index)? - started));
        if !steps.is_empty() {
            blocks.push(Block::Turn {
                opener: chat.id(at),
                steps,
                took,
            });
        }
        for (index, item) in items.iter().enumerate().take(end).skip(at + 1) {
            if Some(index) == answer {
                blocks.push(Block::One(chat.id(index), answer_part));
            } else if !folds_now(item) {
                blocks.push(Block::One(chat.id(index), Part::Whole));
            }
        }
        at = end;
    }
    blocks
}

use dusk_protocol::thread::duration_words as duration;

#[component]
pub fn Transcript(
    source: Source,
    thread: Option<String>,
    /// Nothing here may be answered: a subagent's chat, which the user only reads.
    #[prop(optional)]
    read_only: bool,
) -> impl IntoView {
    let scroller = NodeRef::<html::Div>::new();
    // The blocks follow the chat's shape alone: streamed text leaves them be.
    let shape = Memo::new(move |_| source.with(|c| (c.shape(), c.working)));
    let all = Memo::new(move |_| {
        shape.track();
        source.with_untracked(blocks)
    });
    let index = Memo::new(move |_| {
        all.with(|all| {
            all.iter()
                .enumerate()
                .map(|(at, block)| (block.key(), at))
                .collect::<HashMap<_, _>>()
        })
    });
    let firsts = Memo::new(move |_| all.with(|all| all.iter().map(Block::key).collect::<Vec<_>>()));
    let chat = source.chat();
    let own = source.agent.is_none();
    let pending = move || {
        let pending = if own {
            chat.with(|c| c.pending.clone())
        } else {
            Vec::new()
        };
        pending
            .into_iter()
            .enumerate()
            .collect::<Vec<(usize, String)>>()
    };
    // Follow the end unless the reader scrolled up.
    Effect::new(move |_| {
        source.track_all();
        source.with(|c| (c.items.len(), c.working));
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
    let empty = move || source.with(|c| c.items.is_empty()) && pending().is_empty();
    let queued_thread = thread.clone();
    view! {
        <div class="transcript" node_ref=scroller>
            <Show when=empty>
                <div class="empty chat-empty"><h2>"What are we working on?"</h2><p>"Ask a question, explore your code, or start something new."</p></div>
            </Show>
            <For each=move || firsts.get() key=|key| *key let:key>
                <BlockView all index key source thread=thread.clone() read_only />
            </For>
            <For each=pending key=|(at, text)| (*at, text.clone()) let:pair>
                <div class="msg user queued">
                    <div class="bubble">{pair.1}</div>
                    <div class="muted">
                        "queued"
                        // The core gives back its newest: only that one can be edited.
                        {
                            let thread = queued_thread.clone();
                            let at = pair.0;
                            move || {
                                let newest = chat.with(|c| c.pending.len()) == at + 1;
                                let thread = thread.clone().filter(|_| newest && !read_only)?;
                                Some(view! {
                                    " · "
                                    <button
                                        class="link quiet"
                                        title="Back into the composer; it is not sent"
                                        on:click=move |_| super::composer::take_back(thread.clone(), chat)
                                    >
                                        "Edit"
                                    </button>
                                })
                            }
                        }
                    </div>
                </div>
            </For>
            // Agents below the chat may work on after its own turn: the row stays for them.
            <Show when=move || source.with(|c| c.work(super::data::now_ms()).is_some())>
                <Working chat />
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
fn BlockView(
    all: Memo<Vec<Block>>,
    index: Memo<HashMap<BlockKey, usize>>,
    key: BlockKey,
    source: Source,
    thread: Option<String>,
    read_only: bool,
) -> impl IntoView {
    let block = Memo::new(move |_| {
        let at = index.with(|index| index.get(&key).copied())?;
        all.with(|all| all.get(at).cloned())
    });
    match key {
        (true, _) => view! { <TurnView block source /> }.into_any(),
        (false, id) => {
            let part = Signal::derive(move || match block.get() {
                Some(Block::One(_, part)) => part,
                _ => Part::Whole,
            });
            view! { <ItemView id part source thread read_only /> }.into_any()
        }
    }
}

/// The revision of the item `id`, following streamed text: what an item's view redraws by.
fn revision(source: Source, id: u64) -> Memo<Option<u64>> {
    Memo::new(move |_| {
        source.track_all();
        source.with(|c| c.index_of(id).map(|at| c.revision(at)))
    })
}

/// The item `id` as it is now, read without following anything.
fn item_now(source: Source, id: u64) -> Option<Item> {
    source.with_untracked(|c| c.index_of(id).map(|at| c.items[at].clone()))
}

/// The words or the thinking of the answer `id`, following its revision.
fn answer_text(source: Source, id: u64, rev: Memo<Option<u64>>, said: bool) -> Signal<String> {
    Signal::derive(move || {
        rev.track();
        source.with_untracked(|c| match c.index_of(id).map(|at| &c.items[at]) {
            Some(Item::Assistant { text, reasoning }) => match said {
                true => text.clone(),
                false => reasoning.clone(),
            },
            _ => String::new(),
        })
    })
}

#[component]
fn ItemView(
    id: u64,
    part: Signal<Part>,
    source: Source,
    thread: Option<String>,
    read_only: bool,
) -> impl IntoView {
    let rev = revision(source, id);
    // An answer is drawn once and its words follow; anything else is drawn again when
    // it changes, which is rare.
    let answer = Memo::new(move |_| {
        rev.track();
        source.with_untracked(|c| {
            c.index_of(id)
                .map(|at| matches!(c.items[at], Item::Assistant { .. }))
        })
    });
    let thread = StoredValue::new(thread);
    move || match answer.get() {
        Some(true) => {
            let text = answer_text(source, id, rev, true);
            let reasoning = answer_text(source, id, rev, false);
            let thinking = Signal::derive(move || part.get() == Part::Whole);
            assistant_view(
                id,
                text,
                reasoning,
                thinking,
                rev,
                source,
                thread.get_value(),
            )
            .into_any()
        }
        Some(false) => (move || {
            rev.track();
            item_now(source, id)
                .map(|item| item_view(id, item, source, thread.get_value(), read_only))
        })
        .into_any(),
        None => ().into_any(),
    }
}

fn item_view(
    id: u64,
    item: Item,
    source: Source,
    thread: Option<String>,
    read_only: bool,
) -> impl IntoView {
    let chat = source.chat();
    match item {
        Item::User { answered, .. } if !answered.is_empty() => view! {
            <div class="msg user"><div class="answered">
                {answered.into_iter().map(|answer| view! {
                    <div class="answered-row">
                        <div>{answer.question.clone()}</div>
                        {answer.lines().into_iter().map(|line| view! {
                            <div class="answered-text">{line}</div>
                        }).collect_view()}
                    </div>
                }).collect_view()}
            </div></div>
        }
        .into_any(),
        Item::User { text, .. } => view! {
            <div class="msg user">
                <div class="bubble">{user_text(&text)}</div>
                <div class="msg-actions">{fork_button(id, source, thread.clone())}</div>
            </div>
        }
        .into_any(),
        Item::Job(result) => {
            let title = result.title();
            let class = if result.failed() {
                "steps error"
            } else {
                "steps"
            };
            view! { <details class=class><summary>{title}</summary><pre class="tool-out">{result.output}</pre></details> }.into_any()
        }
        Item::Agent { from, text, .. } => view! {
            <div class="msg agent">
                <div class="muted">{format!("From {from}")}</div>
                <Markdown text=text />
            </div>
        }
        .into_any(),
        // Drawn by `ItemView` with words that follow the stream.
        Item::Assistant { .. } | Item::Tool(_) | Item::Local(_) => ().into_any(),
        Item::Summary(text) => {
            view! { <div class="msg assistant"><Markdown text=text /></div> }.into_any()
        }
        Item::Block(block) => view! { <ShownBlock block id chat thread read_only /> }.into_any(),
        Item::Suggestion(card) => {
            view! { <crate::pages::customize::SuggestedCard card /> }.into_any()
        }
        Item::Proposal { proposal, state } => {
            view! { <ProposalCard proposal settled=state thread read_only /> }.into_any()
        }
        Item::Image { card, .. } => view! { <ImageCard card /> }.into_any(),
        Item::Audio(card) => view! { <AudioCard card /> }.into_any(),
        Item::Run(_) => view! { <RunCardView id source /> }.into_any(),
        Item::Notice(text) => view! { <div class="notice">{text}</div> }.into_any(),
        // The evaluator's verdict on the goal; its reason unfolds under it.
        Item::Goal(evaluation) => {
            let tone = match evaluation.verdict {
                GoalVerdict::NotMet => "",
                GoalVerdict::Met => "good",
                GoalVerdict::Impossible => "bad",
            };
            view! {
                <details class=format!("notice goal-verdict {tone}")>
                    <summary>{format!("\u{25CE} {}", evaluation.title())}</summary>
                    <div class="goal-reason">{evaluation.reason}</div>
                </details>
            }
            .into_any()
        }
        Item::Error(text) => view! { <ErrorRow id text source thread /> }.into_any(),
    }
}

/// A message as the user wrote it: each paste and kept file as a short label.
fn user_text(text: &str) -> String {
    labelled(text, |pasted| {
        format!("pasted {} lines", pasted.lines().count().max(1))
    })
}

/// A new chat from the history up to this item; none while its stored message is not
/// known yet, as an answer still being written.
fn fork_button(id: u64, source: Source, thread: Option<String>) -> Option<impl IntoView> {
    let message_id = source.with_untracked(|c| {
        c.index_of(id)
            .and_then(|at| c.message_id(at))
            .map(str::to_owned)
    })?;
    let thread = thread?;
    Some(view! {
        <button class="icon" title="Fork from here" aria-label="Fork from here"
            on:click=move |_| state().fork_at(thread.clone(), message_id.clone())>
            <Icon name=IconName::GitFork />
        </button>
    })
}

fn assistant_view(
    id: u64,
    text: Signal<String>,
    reasoning: Signal<String>,
    thinking: Signal<bool>,
    rev: Memo<Option<u64>>,
    source: Source,
    thread: Option<String>,
) -> impl IntoView {
    let copied = RwSignal::new(false);
    let copy_answer = move |_| {
        copy(&text.get_untracked());
        copied.set(true);
        leptos::task::spawn_local(async move {
            crate::ui::sleep(1500).await;
            copied.set(false);
        });
    };
    let thread = StoredValue::new(thread);
    // The answer's ID comes as its turn ends; the fork button with it.
    let fork = move || {
        rev.track();
        fork_button(id, source, thread.get_value())
    };
    // The answer being written has a soft caret at its growing end.
    let writing = move || {
        source.with(|c| {
            c.writing(super::data::now_ms())
                .is_some_and(|at| c.id(at) == id)
        })
    };
    view! {
        <div class="msg assistant" class:writing=writing>
            <Show when=move || thinking.get() && !reasoning.with(String::is_empty)>
                <details class="reasoning">
                    <summary>"Thinking"</summary>
                    <Markdown text=reasoning />
                </details>
            </Show>
            <Markdown text=text marks=true />
            <div class="msg-actions">
                <button class="icon" class:copied=move || copied.get() title="Copy the answer" aria-label="Copy the answer" on:click=copy_answer>
                    {move || if copied.get() {
                        view! { <Icon name=IconName::Check /> }.into_any()
                    } else {
                        view! { <Icon name=IconName::Copy /> }.into_any()
                    }}
                </button>
                <button class="icon" title="Read aloud" on:click=move |_| voice::speak(text.get_untracked())>
                    <Icon name=IconName::Play />"Listen"
                </button>
                {fork}
            </div>
        </div>
    }
}

pub(super) fn copy(text: &str) {
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

/// What a turn's fold line says: how long it took or what it does now, the tools by
/// name, and how many failed.
#[derive(Clone, PartialEq)]
struct Heading {
    running: bool,
    title: String,
    names: String,
    failed: usize,
}

fn heading(chat: &ChatLog, steps: &[(u64, Part)], took: Option<i64>) -> Heading {
    let tools: Vec<&Tool> = steps
        .iter()
        .filter_map(
            |(id, _)| match chat.index_of(*id).map(|at| &chat.items[at]) {
                Some(Item::Tool(tool)) => Some(tool),
                _ => None,
            },
        )
        .collect();
    let title = match took {
        Some(ms) => format!("Worked for {}", duration(ms)),
        None => tools
            .iter()
            .rev()
            .map(|tool| match tool.detail.trim().is_empty() {
                true => tool.name.clone(),
                false => tool.detail.clone(),
            })
            .next()
            .unwrap_or_else(|| "Thinking".to_owned()),
    };
    Heading {
        // A step that runs marks the turn's line; what else the turn does, the row
        // below says.
        running: took.is_none()
            && chat.working
            && tools.iter().any(|tool| tool.state == ToolState::Running),
        title,
        names: summary_line(&tools),
        failed: tools
            .iter()
            .filter(|tool| tool.state == ToolState::Failed)
            .count(),
    }
}

/// A turn's way to its answer: one line that says how long it took, or what it does now,
/// and under it every step, thought and remark. The fold is drawn once: steps come and
/// change under it, and the reader's open or closed stays.
#[component]
fn TurnView(block: Memo<Option<Block>>, source: Source) -> impl IntoView {
    let steps = Memo::new(move |_| match block.get() {
        Some(Block::Turn { steps, .. }) => steps,
        _ => Vec::new(),
    });
    let took = Memo::new(move |_| match block.get() {
        Some(Block::Turn { took, .. }) => took,
        _ => None,
    });
    let head = Memo::new(move |_| {
        let took = took.get();
        steps.with(|steps| source.with(|c| heading(c, steps, took)))
    });
    let title = Memo::new(move |_| head.with(|h| h.title.clone()));
    let drawn = StoredValue::new(false);
    // A folded turn still shows what its steps changed in skills, as the desktop does.
    let calls = move || {
        steps.with(|steps| {
            source.with(|c| {
                steps
                    .iter()
                    .filter_map(|(id, _)| match c.index_of(*id).map(|at| &c.items[at]) {
                        Some(Item::Tool(tool)) => Some(tool.call_id.clone()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
            })
        })
    };
    view! {
        <details class="steps">
            <summary>
                {move || head.with(|h| h.running).then(|| view! { <span class="act-slot" aria-hidden="true"><span class="act-live"></span></span> })}
                {move || {
                    // A phrase that takes another's place comes in softly; the one a
                    // turn is first drawn with simply stands.
                    let title = title.get();
                    let coming = drawn.get_value();
                    drawn.set_value(true);
                    view! { <span class="steps-title" class:coming=coming>{title}</span> }
                }}
                <span class="muted">{move || head.with(|h| h.names.clone())}</span>
                {move || {
                    let failed = head.with(|h| h.failed);
                    (failed > 0).then(|| view! { <span class="bad">{format!("{failed} failed")}</span> })
                }}
            </summary>
            <For each=move || steps.get() key=|step| *step let:step>
                <StepView id=step.0 part=step.1 source />
            </For>
        </details>
        <For each=calls key=|call_id| call_id.clone() let:call_id>
            <SkillChanges call_id source />
        </For>
    }
}

#[component]
fn StepView(id: u64, part: Part, source: Source) -> impl IntoView {
    let rev = revision(source, id);
    let answer = Memo::new(move |_| {
        rev.track();
        source.with_untracked(|c| {
            c.index_of(id).map(|at| match &c.items[at] {
                Item::Assistant { .. } => 1,
                Item::Tool(_) => 2,
                _ => 0,
            })
        })
    });
    move || {
        match answer.get() {
        Some(1) => {
            let text = answer_text(source, id, rev, true);
            let reasoning = answer_text(source, id, rev, false);
            let said = part != Part::Thought;
            view! {
                <div class="step-said">
                    <Show when=move || !reasoning.with(|r| r.trim().is_empty())>
                        <details class="reasoning"><summary>"Thinking"</summary><Markdown text=reasoning /></details>
                    </Show>
                    <Show when=move || said && !text.with(|t| t.trim().is_empty())>
                        <Markdown text marks=true />
                    </Show>
                </div>
            }
            .into_any()
        }
        Some(2) => {
            let tool = Memo::new(move |_| {
                rev.track();
                match item_now(source, id) {
                    Some(Item::Tool(tool)) => Some(tool),
                    _ => None,
                }
            });
            view! { <ToolStep tool /> }.into_any()
        }
        Some(_) => (move || {
            rev.track();
            item_now(source, id).map(|item| match item {
                Item::Agent { from, text, .. } => view! {
                    <div class="step-said"><div class="muted">{format!("From {from}")}</div><Markdown text=text /></div>
                }.into_any(),
                Item::Image { card, .. } => view! { <ImageCard card /> }.into_any(),
                Item::Audio(card) => view! { <AudioCard card /> }.into_any(),
                Item::Notice(text) | Item::Summary(text) => {
                    view! { <div class="step-said muted">{text}</div> }.into_any()
                }
                _ => ().into_any(),
            })
        })
        .into_any(),
        None => ().into_any(),
    }
    }
}

/// The tools of a run by name with counts, such as `bash x3, read x2`.
fn summary_line(tools: &[&Tool]) -> String {
    let mut counts: Vec<(String, usize)> = Vec::new();
    for tool in tools {
        match counts.iter_mut().find(|(name, _)| *name == tool.name) {
            Some((_, n)) => *n += 1,
            None => counts.push((tool.name.clone(), 1)),
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

/// One step, drawn once: its state and what it answered follow, so a step the reader
/// opened stays open as it finishes.
#[component]
fn ToolStep(tool: Memo<Option<Tool>>) -> impl IntoView {
    let field = move |get: fn(&Tool) -> String| {
        move || tool.with(|t| t.as_ref().map(get).unwrap_or_default())
    };
    let state_class = move || {
        let state = tool.with(|t| t.as_ref().map(|t| t.state));
        match state {
            Some(ToolState::Running) | None => "tool running",
            Some(ToolState::Done) => "tool done",
            Some(ToolState::Failed) => "tool failed",
        }
    };
    let name_class = move || {
        let agent = tool.with(|t| {
            t.as_ref()
                .is_some_and(|t| dusk_protocol::tools::is_agent_tool(&t.name))
        });
        if agent {
            "tool-name agent"
        } else {
            "tool-name"
        }
    };
    let phrase = field(|t| dusk_protocol::tools::phrase(&t.name, &t.call_id).to_owned());
    let output = field(|t| t.output.clone());
    // The state at the end of the line: a live mark while it runs, then a check or a
    // cross. One that finishes while it is on the page settles in once; one that was
    // finished when it was drawn simply stands.
    let state = Memo::new(move |_| tool.with(|t| t.as_ref().map(|t| t.state)));
    let ran = StoredValue::new(false);
    let mark = move || {
        let settles = ran.get_value();
        let class = if settles { "glyph-settle" } else { "" };
        match state.get() {
            Some(ToolState::Running) | None => {
                ran.set_value(true);
                view! { <span class="act-live"></span> }.into_any()
            }
            Some(ToolState::Done) => {
                view! { <span class=class><Icon name=IconName::Check /></span> }.into_any()
            }
            Some(ToolState::Failed) => {
                view! { <span class=class><Icon name=IconName::Close /></span> }.into_any()
            }
        }
    };
    view! {
        <details class=state_class>
            <summary>
                <span class=name_class>{phrase}</span>
                <span class="muted">{field(|t| t.detail.clone())}</span>
                <Show when=move || state.get() == Some(ToolState::Failed)>
                    <span class="tool-failed">"Failed"</span>
                </Show>
                <span class="tool-mark" aria-hidden="true">{mark}</span>
            </summary>
            <div class="muted mono">{field(|t| t.name.clone())}</div>
            <pre class="tool-args">{field(|t| t.arguments.clone())}</pre>
            {move || {
                let output = output();
                (!output.is_empty()).then(|| view! { <pre class="tool-out">{output}</pre> })
            }}
        </details>
    }
}

#[component]
fn ImageCard(card: Card) -> impl IntoView {
    let alt = card
        .title
        .clone()
        .or(card.description.clone())
        .unwrap_or_else(|| "picture".to_owned());
    view! {
        <figure class="card">
            <img src=format!("/api/card/{}/image", card.id) alt=alt.clone() loading="lazy" />
            {card.description.map(|d| view! { <figcaption class="muted">{d}</figcaption> })}
        </figure>
    }
}

#[component]
fn AudioCard(card: Card) -> impl IntoView {
    view! {
        <div class="card">
            <div class="muted">{card.title.clone().unwrap_or_else(|| "Sound".to_owned())}</div>
            <audio controls src=format!("/api/card/{}/audio", card.id)></audio>
        </div>
    }
}

/// A failure said quietly: the words in the text color on a raised row with a thin edge
/// in the danger color. The error the chat ends with while no turn runs offers to send
/// the last message once more.
#[component]
fn ErrorRow(id: u64, text: String, source: Source, thread: Option<String>) -> impl IntoView {
    let last_sent = move || {
        source.with(|c| {
            let last = c.items.len().checked_sub(1)?;
            let ends_here = !c.working && c.id(last) == id;
            ends_here.then(|| {
                c.items.iter().rev().find_map(|item| match item {
                    Item::User { text, .. } => Some(text.clone()),
                    _ => None,
                })
            })?
        })
    };
    let thread = StoredValue::new(thread);
    view! {
        <div class="error-row" role="alert">
            <Icon name=IconName::TriangleAlert />
            <span class="error-text">{text}</span>
            {move || thread.get_value().and(last_sent()).map(|again| view! {
                <button class="ghost quiet" title="Send your last message once more" on:click=move |_| {
                    crate::state::state().send(thread.get_value(), again.clone(), false, Vec::new());
                }>"Send again"</button>
            })}
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dusk_protocol::{
        model::{ModelEvent, ToolCall},
        thread::{ThreadEvent, Turn, TurnStatus},
    };

    fn running_turn() -> Turn {
        Turn {
            turn_id: "turn".into(),
            thread_id: "thread".into(),
            last_sequence: 1,
            response_message_id: None,
            status: TurnStatus::Running,
            started_at: 0,
            finished_at: None,
            error_message: None,
            waiting: None,
            steps: 0,
            tokens: 0,
            cost_micro_usd: 0,
        }
    }

    fn said(chat: &ChatLog, block: &Block) -> Option<String> {
        let Block::One(id, _) = block else {
            return None;
        };
        match &chat.items[chat.index_of(*id)?] {
            Item::Assistant { text, .. } => Some(text.clone()),
            _ => None,
        }
    }

    /// A turn of thinking and an answer, with no tool: the fold holds only the answer's
    /// thinking. Two blocks under one key made the list draw the fold twice and never
    /// the answer.
    #[test]
    fn a_turn_of_thought_and_answer_draws_each_block_once() {
        let mut chat = ChatLog::default();
        // Any item opens the turn the way the user's message does.
        chat.apply(
            ThreadEvent::Rejected {
                reason: "first".into(),
            },
            0,
        );
        chat.apply(ThreadEvent::Model(ModelEvent::Reasoning("hm".into())), 0);
        chat.apply(ThreadEvent::Model(ModelEvent::Text("done".into())), 0);
        let blocks = blocks(&chat);
        assert_eq!(blocks.len(), 3);
        assert!(matches!(&blocks[1], Block::Turn { .. }));
        assert_eq!(said(&chat, &blocks[2]).as_deref(), Some("done"));
        assert!(matches!(blocks[2], Block::One(_, Part::Said)));
        assert_ne!(blocks[1].key(), blocks[2].key());
    }

    /// A client that joins a turn in flight must keep its earlier remarks in sight: it
    /// missed the turn's start, but the fold of a finished turn would hide them.
    #[test]
    fn a_turn_joined_midway_keeps_its_earlier_remark_in_sight() {
        let call = ToolCall {
            id: "call".into(),
            name: "bash".into(),
            arguments: serde_json::json!({ "command": "ls" }),
        };
        let mut chat = ChatLog::default();
        chat.add_user("go".into(), Some(1), 1);
        chat.push(
            Item::Assistant {
                text: "first".into(),
                reasoning: String::new(),
            },
            2,
        );
        chat.push(Item::Tool(Tool::new(&call, ToolState::Done)), 3);
        chat.apply(ThreadEvent::TurnRunning(running_turn()), 4);
        chat.apply(ThreadEvent::Model(ModelEvent::Text("second".into())), 4);
        let blocks = blocks(&chat);
        let texts: Vec<String> = blocks.iter().filter_map(|b| said(&chat, b)).collect();
        assert_eq!(texts, ["first", "second"]);
    }

    /// The goal's verdict comes after a turn and is the user's to read: the finished
    /// turn's fold must not take it in, where nothing draws it.
    #[test]
    fn a_goal_verdict_stays_out_of_the_finished_turn() {
        use dusk_protocol::thread::GoalEvaluation;
        let mut chat = ChatLog::default();
        chat.add_user("go".into(), Some(1), 1);
        chat.apply(ThreadEvent::Model(ModelEvent::Reasoning("hm".into())), 2);
        chat.apply(ThreadEvent::Model(ModelEvent::Text("done".into())), 2);
        let verdict = GoalEvaluation {
            verdict: GoalVerdict::NotMet,
            reason: "run the tests".into(),
            turn: 1,
            tokens: 10,
            elapsed_ms: 10,
        };
        chat.apply(ThreadEvent::GoalEvaluated(verdict.clone()), 3);
        let blocks = blocks(&chat);
        assert!(blocks.iter().any(|block| matches!(block, Block::One(id, _)
            if chat.items[chat.index_of(*id).unwrap()] == Item::Goal(verdict.clone()))));
    }

    /// A block is what the agent showed, not a step on its way: a finished turn keeps it
    /// in sight with the answer.
    #[test]
    fn a_finished_turn_keeps_its_block_in_sight() {
        let call = ToolCall {
            id: "show".into(),
            name: dusk_protocol::thread::SHOW_TOOL.into(),
            arguments: serde_json::json!({ "choices": { "labels": ["Yes", "No"] } }),
        };
        let shown = dusk_protocol::thread::Block::asked(&call).expect("a block");
        let mut chat = ChatLog::default();
        chat.apply(
            ThreadEvent::Rejected {
                reason: "first".into(),
            },
            0,
        );
        chat.apply(ThreadEvent::ToolStarted(call), 0);
        chat.apply(
            ThreadEvent::ToolFinished {
                call_id: "show".into(),
                result: dusk_protocol::tools::ToolResult::text(shown.text()),
            },
            0,
        );
        chat.apply(ThreadEvent::Model(ModelEvent::Text("Pick one".into())), 0);
        let blocks = blocks(&chat);
        assert!(blocks.iter().any(|block| matches!(block, Block::One(id, _)
            if matches!(&chat.items[chat.index_of(*id).unwrap()], Item::Block(block) if *block == shown))));
        assert!(
            !blocks
                .iter()
                .any(|block| matches!(block, Block::Turn { .. }))
        );
    }

    /// A Workshop card is what the agent showed too: a finished turn keeps it in sight,
    /// where its rows can still be read and opened.
    #[test]
    fn a_finished_turn_keeps_its_workshop_card_in_sight() {
        use dusk_protocol::plugin::{Listing, SUGGEST_TOOL, Suggested, Suggestion};
        let card = Suggestion {
            label: "Design skills".into(),
            listings: vec![Suggested::of(&Listing {
                source: "official".into(),
                name: "motion".into(),
                ..Listing::default()
            })],
        };
        let call = ToolCall {
            id: "card".into(),
            name: SUGGEST_TOOL.into(),
            arguments: serde_json::json!({ "ids": ["official/motion"], "label": "x" }),
        };
        let mut chat = ChatLog::default();
        chat.add_user("go".into(), Some(1), 0);
        chat.apply(ThreadEvent::TurnStarted(running_turn()), 0);
        chat.apply(ThreadEvent::ToolStarted(call), 0);
        chat.apply(
            ThreadEvent::ToolFinished {
                call_id: "card".into(),
                result: dusk_protocol::tools::ToolResult::text(card.result()),
            },
            0,
        );
        chat.apply(ThreadEvent::Model(ModelEvent::Text("Here".into())), 0);
        chat.apply(ThreadEvent::Idle, 0);
        let blocks = blocks(&chat);
        assert!(blocks.iter().any(|block| matches!(block, Block::One(id, _)
            if matches!(&chat.items[chat.index_of(*id).unwrap()], Item::Suggestion(shown) if *shown == card))));
    }

    /// Streamed text changes no block, so the running turn's fold is not drawn again and
    /// a fold the reader opened stays open.
    #[test]
    fn streamed_text_leaves_the_blocks_as_they_are() {
        let call = ToolCall {
            id: "call".into(),
            name: "bash".into(),
            arguments: serde_json::json!({ "command": "ls" }),
        };
        let mut chat = ChatLog::default();
        chat.add_user("go".into(), Some(1), 0);
        chat.apply(ThreadEvent::TurnStarted(running_turn()), 0);
        chat.apply(
            ThreadEvent::Model(ModelEvent::Reasoning("let me".into())),
            0,
        );
        chat.apply(ThreadEvent::ToolStarted(call), 0);
        chat.apply(ThreadEvent::Model(ModelEvent::Reasoning("so".into())), 0);
        let (shape, before) = (chat.shape(), blocks(&chat));
        for piece in [" then", " more"] {
            let applied = chat.apply(ThreadEvent::Model(ModelEvent::Reasoning(piece.into())), 0);
            assert!(matches!(applied, dusk_protocol::thread::Applied::Item(_)));
        }
        assert_eq!(chat.shape(), shape);
        assert_eq!(blocks(&chat), before);
    }

    /// A fold keeps its name from the turn's start to its end, though what it holds
    /// changes: a remark before the first step joins it only once the turn is over.
    #[test]
    fn a_fold_keeps_its_name_as_its_turn_ends() {
        let call = ToolCall {
            id: "call".into(),
            name: "bash".into(),
            arguments: serde_json::json!({ "command": "ls" }),
        };
        let fold = |chat: &ChatLog| {
            blocks(chat)
                .iter()
                .find(|block| matches!(block, Block::Turn { .. }))
                .map(Block::key)
        };
        let mut chat = ChatLog::default();
        chat.add_user("go".into(), Some(1), 0);
        chat.apply(ThreadEvent::TurnStarted(running_turn()), 0);
        chat.apply(
            ThreadEvent::Model(ModelEvent::Text("Let me look.".into())),
            0,
        );
        chat.apply(ThreadEvent::ToolStarted(call), 0);
        let running = fold(&chat);
        assert!(running.is_some());
        chat.apply(ThreadEvent::Model(ModelEvent::Text("Found it.".into())), 0);
        chat.apply(ThreadEvent::TurnFinished(running_turn()), 0);
        assert_eq!(fold(&chat), running);
    }
}

/// A team or subagents the chat started, as a card where it began: its title, "Team ·
/// 5 agents · 1m 12s" and a square a task or agent. A click opens its tab in the right
/// column: the tasks for a team, the agents for subagents. While it runs, its clock is
/// read once a second, and not while the page is hidden.
#[component]
fn RunCardView(id: u64, source: Source) -> impl IntoView {
    use dusk_protocol::thread::{RunKind, RunMark};
    let now = RwSignal::new(super::data::now_ms());
    let card = Memo::new(move |_| {
        source.with(|c| c.index_of(id).and_then(|at| c.run_card(at, now.get())))
    });
    let handle = set_interval_with_handle(
        move || {
            let hidden = web_sys::window()
                .and_then(|w| w.document())
                .is_some_and(|d| d.hidden());
            let running =
                card.with_untracked(|card| card.as_ref().is_some_and(|card| card.running));
            if running && !hidden {
                now.set(super::data::now_ms());
            }
        },
        std::time::Duration::from_secs(1),
    )
    .ok();
    on_cleanup(move || {
        if let Some(handle) = handle {
            handle.clear();
        }
    });
    move || {
        let card = card.get()?;
        let tab = match card.kind {
            RunKind::Team => super::inspector::Tab::Tasks,
            RunKind::Subagents => super::inspector::Tab::Agents,
        };
        let marks = card
            .marks
            .iter()
            .map(|mark| {
                let class = match mark {
                    RunMark::Done => "run-mark done",
                    RunMark::Running => "run-mark running",
                    RunMark::Waiting | RunMark::Confirm => "run-mark waiting",
                    RunMark::Open => "run-mark open",
                    RunMark::Blocked => "run-mark blocked",
                    RunMark::Failed => "run-mark failed",
                };
                view! { <i class=class></i> }
            })
            .collect_view();
        let line = card.line();
        let shown = !card.marks.is_empty();
        // The row takes the chat's lane, so the card stands at its start at any width.
        Some(view! {
            <div class="run-row">
                <button class="run-card" title=card.title.clone()
                    on:click=move |_| state().show_inspector(tab)>
                    <span class="run-card-head">
                        <span class="run-card-title">{card.title.clone()}</span>
                        <Icon name=IconName::ChevronRight />
                    </span>
                    <span class="run-card-line">{line}</span>
                    {shown.then(|| view! { <span class="run-marks" aria-hidden="true">{marks}</span> })}
                </button>
            </div>
        })
    }
}

/// A session the agent proposed: its title, the brief's first lines that open in full,
/// and Start or Dismiss while it waits; afterwards where it went.
#[component]
fn ProposalCard(
    proposal: dusk_protocol::thread::Proposal,
    settled: dusk_protocol::thread::ProposalState,
    thread: Option<String>,
    read_only: bool,
) -> impl IntoView {
    use dusk_protocol::thread::ProposalState;
    let lines: Vec<&str> = proposal.brief.lines().collect();
    let head = lines.iter().take(3).copied().collect::<Vec<_>>().join("\n");
    let more = lines.len() > 3;
    let mut about = Vec::new();
    about.extend(proposal.mode.clone().map(|mode| format!("mode {mode}")));
    about.extend(
        proposal
            .branch
            .clone()
            .map(|branch| format!("branch {branch}")),
    );
    about.extend(
        proposal
            .project
            .clone()
            .map(|project| format!("in {project}")),
    );
    let about = about.join(" \u{b7} ");
    let actions = match (settled, thread) {
        (ProposalState::Pending, Some(thread)) if !read_only => {
            let id = proposal.id.clone();
            let (start_thread, start_id) = (thread.clone(), id.clone());
            view! {
                <div class="proposal-actions">
                    <button class="primary" on:click=move |_| {
                        state().start_proposal(start_thread.clone(), start_id.clone())
                    }>"Start session"</button>
                    <button class="ghost" on:click=move |_| {
                        state().dismiss_proposal(thread.clone(), id.clone())
                    }>"Dismiss"</button>
                </div>
            }
            .into_any()
        }
        (ProposalState::Pending, _) => {
            view! { <p class="proposal-state">"Waiting for the user"</p> }.into_any()
        }
        (ProposalState::Started { thread_id }, _) => view! {
            <button class="ghost proposal-state" on:click=move |_| {
                state().show(View::Chat(Some(thread_id.clone())))
            }>"Started \u{2192} open the session"</button>
        }
        .into_any(),
        (ProposalState::Dismissed, _) => {
            view! { <p class="proposal-state">"Dismissed"</p> }.into_any()
        }
    };
    let brief = proposal.brief.clone();
    view! {
        <section class="proposal" aria-label="Proposed session">
            <p class="proposal-kicker">"Proposed session"</p>
            <h3>{proposal.title}</h3>
            {(!about.is_empty()).then(|| view! { <p class="proposal-about">{about}</p> })}
            {match more {
                true => view! {
                    <details class="proposal-brief">
                        <summary>{head}</summary>
                        <pre>{brief}</pre>
                    </details>
                }
                .into_any(),
                false => view! { <pre class="proposal-brief">{brief}</pre> }.into_any(),
            }}
            {actions}
        </section>
    }
}
