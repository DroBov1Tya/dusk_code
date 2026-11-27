//! A subagent's chat, to read: what it was asked, what it said and the steps it took,
//! live while it works. It is the parent chat's to steer, so there is no composer.

use dusk_protocol::thread::AgentState;
use gpui_kit::{
    component::{
        ActiveTheme, IconName, Sizable,
        button::{Button, ButtonVariants},
    },
    prelude::FluentBuilder,
    *,
};

use super::Modal;
use crate::{
    columns::inspector::mascot,
    pages::chat::{
        messages::{
            Row, Texts, activity_text, agent_report, notice, row_of, run, selectable, steps,
            sync_list,
        },
        transcript::{Item, Log},
    },
    ui::{Type, widgets},
    workspace::Workspace,
};

/// The rows of a subagent's chat window, drawn as a list as the chat's are: only the rows
/// in sight are laid out and their heights kept, so a chat of hundreds of steps scrolls,
/// and redraws for a mascot's tick, without building every row.
pub struct AgentList {
    pub list: ListState,
    /// The agent shown; another one, or the same one opened again, starts at its end.
    pub agent: Option<String>,
    /// The chat's shape and whether it works, which the rows were built from.
    key: Option<(u64, bool)>,
    rows: Vec<Row>,
    /// The parsed words of the rows, kept while the agent is shown.
    texts: Texts,
    /// A row was unfolded or folded: every row is measured again.
    pub measure_again: bool,
}

impl Default for AgentList {
    fn default() -> Self {
        Self {
            list: ListState::new(0, ListAlignment::Top, px(800.)),
            agent: None,
            key: None,
            rows: Vec::new(),
            texts: Texts::of(Workspace::agent_text_changed),
            measure_again: false,
        }
    }
}

pub fn render(ws: &mut Workspace, agent: &str, cx: &mut Context<Workspace>) -> Div {
    let mut column = div().flex_1().min_h_0().flex().flex_col();
    column = column.children(head(ws, agent, cx).map(|head| div().px_5().pt_5().child(head)));
    let shown = ws.shown.as_deref().unwrap_or_default();
    let Some(chat) = ws
        .chats
        .get_mut(shown)
        .and_then(|chat| chat.agent_chats.get_mut(agent))
    else {
        return column.child(div().p_5().child(widgets::muted("Loading...", cx)));
    };
    if chat.items.is_empty() {
        return column.child(div().p_5().child(widgets::muted("Nothing yet.", cx)));
    }
    // The rows change only with the chat's shape: streamed text and timers keep them.
    let key = (chat.shape(), chat.working);
    let changed = chat.take_changed();
    let state = &mut ws.agent_list;
    let old = std::mem::take(&mut state.rows);
    if state.agent.as_deref() != Some(agent) {
        state.rows = rows(chat);
        state.agent = Some(agent.to_owned());
        state.texts = Texts::of(Workspace::agent_text_changed);
        state.measure_again = false;
        // It opens at its latest message and follows new ones while read at the end.
        state.list.reset(state.rows.len());
        state.list.set_follow_mode(FollowMode::Tail);
    } else if state.key != Some(key) {
        state.rows = rows(chat);
        sync_list(
            &state.list,
            &old,
            &state.rows,
            changed,
            &mut state.measure_again,
        );
    } else {
        sync_list(&state.list, &old, &old, changed, &mut state.measure_again);
        state.rows = old;
    }
    state.key = Some(key);
    column.child(
        list(
            state.list.clone(),
            cx.processor(|ws, ix: usize, _window, cx| ws.agent_row(ix, cx)),
        )
        .with_sizing_behavior(ListSizingBehavior::Infer)
        .flex_1()
        .min_h_0(),
    )
}

/// The rows of a subagent's chat: its items, a run of steps folded into one as in the
/// chat, and the working row while it works.
fn rows(chat: &Log) -> Vec<Row> {
    let mut rows = Vec::new();
    let mut index = 0;
    while index < chat.items.len() {
        if let Item::Tool(_) = chat.items[index] {
            let start = index;
            while index < chat.items.len() && matches!(chat.items[index], Item::Tool(_)) {
                index += 1;
            }
            if !steps(&chat.items[start..index]).is_empty() {
                rows.push(Row::Activity {
                    start,
                    end: index,
                    span: None,
                    spent: None,
                });
            }
            continue;
        }
        // Only a chat shows Workshop cards and has a goal; an agent has no user.
        if !matches!(
            chat.items[index],
            Item::Suggestion(_) | Item::Proposal { .. } | Item::Goal(_)
        ) {
            rows.push(Row::Item(index));
        }
        index += 1;
    }
    if chat.work(dusk_protocol::system::now_ms()).is_some() {
        rows.push(Row::Working);
    }
    rows
}

impl Workspace {
    /// One row of the shown subagent's chat; space between rows lives in the rows.
    fn agent_row(&mut self, ix: usize, cx: &mut Context<Self>) -> AnyElement {
        let ws: &Workspace = self;
        let (Some(Modal::AgentChat { agent, .. }), Some(row)) =
            (&ws.modal, ws.agent_list.rows.get(ix).copied())
        else {
            return div().into_any_element();
        };
        let Some(chat) = ws
            .shown_chat()
            .and_then(|chat| chat.agent_chats.get(agent.as_str()))
        else {
            return div().into_any_element();
        };
        let last = ix + 1 == ws.agent_list.rows.len();
        div()
            .px_5()
            .pt_3()
            .when(last, |row| row.pb_5())
            .child(agent_item(ws, agent, chat, row, cx))
            .into_any_element()
    }

    /// Measures again the row of the subagent's chat that shows item `item`, as when a
    /// long parse of its words lands.
    fn agent_text_changed(&mut self, item: usize, cx: &mut Context<Self>) {
        if let Some(row) = row_of(&self.agent_list.rows, item) {
            self.agent_list.list.remeasure_items(row..row + 1);
            cx.notify();
        }
    }
}

fn agent_item(
    ws: &Workspace,
    agent: &str,
    chat: &Log,
    row: Row,
    cx: &mut Context<Workspace>,
) -> AnyElement {
    let theme = cx.theme();
    let (muted, surface, danger) = (theme.muted_foreground, theme.muted, theme.danger);
    let texts = &ws.agent_list.texts;
    let at = match row {
        Row::Item(at) => at,
        // The steps are folded as in the chat: the latest shows, the earlier ones unfold.
        Row::Activity { start, end, .. } => {
            let tools = steps(&chat.items[start..end]);
            return run(ws, agent, start, &tools, cx).into_any_element();
        }
        Row::Working => {
            use crate::pages::chat::activity;
            let Some(work) = chat.work(dusk_protocol::system::now_ms()) else {
                return div().into_any_element();
            };
            return div()
                .h(px(28.))
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .meta()
                .text_color(muted)
                .child(activity::mark(&work, cx))
                .child(activity::words(&work, cx))
                .into_any_element();
        }
    };
    let key = |part: &str| SharedString::from(format!("agent-{part}-{agent}-{at}"));
    // Every message copies whole, from a button that comes up under the pointer.
    let group = key("row");
    let copy = |text: &str, cx: &App| {
        div()
            .flex()
            .justify_end()
            .child(widgets::copy(key("copy"), &group, text, cx))
    };
    match &chat.items[at] {
        // What it was asked reads as it was written: lists, code and all.
        Item::User { text, .. } => div()
            .group(group.clone())
            .px_3()
            .py_2()
            .rounded_lg()
            .bg(surface)
            .small()
            .child(texts.message(key("asked"), at, text, cx))
            .child(copy(text, cx))
            .into_any_element(),
        Item::Agent { from, text, .. } => {
            agent_report(ws, agent, at, (from, text), Some(texts), cx).into_any_element()
        }
        Item::Assistant { text, reasoning } => {
            let mut said = div().flex().flex_col().gap_2();
            if !reasoning.trim().is_empty() {
                said = said.child(activity_text(
                    ws,
                    agent,
                    at,
                    ("Thinking", IconName::Asterisk),
                    reasoning,
                    || selectable(format!("agent-thought-{agent}-{at}"), reasoning),
                    cx,
                ));
            }
            if !text.is_empty() {
                said = said
                    .group(group.clone())
                    .child(texts.answer(key("said"), at, text, cx))
                    .child(copy(text, cx));
            }
            said.into_any_element()
        }
        Item::Tool(_) | Item::Suggestion(_) | Item::Proposal { .. } | Item::Goal(_) => {
            div().into_any_element()
        }
        // Agents an agent started read as one quiet line; their details are the chat's.
        Item::Run(_) => div()
            .meta()
            .text_color(muted)
            .children(
                chat.run_card(at, dusk_protocol::system::now_ms())
                    .map(|card| format!("{} \u{00B7} {}", card.title, card.line())),
            )
            .into_any_element(),
        Item::Block(shown) => crate::pages::chat::messages::block(ws, agent, at, shown, cx),
        Item::Job(result) => {
            crate::pages::chat::messages::job(ws, agent, at, result, cx).into_any_element()
        }
        Item::Local(run) => div()
            .meta()
            .text_color(muted)
            .child(format!("$ {}", run.command))
            .into_any_element(),
        Item::Audio(card) => div()
            .meta()
            .text_color(muted)
            .child(card.title.clone().unwrap_or_else(|| "A sound".to_owned()))
            .into_any_element(),
        Item::Image { card: picture, .. } => div()
            .meta()
            .text_color(muted)
            .child(
                picture
                    .title
                    .clone()
                    .unwrap_or_else(|| "A picture".to_owned()),
            )
            .into_any_element(),
        Item::Notice(text) | Item::Summary(text) => {
            notice(&key("notice"), div().meta().text_color(muted), text, cx).into_any_element()
        }
        // What went wrong must not read as one more quiet note.
        Item::Error(text) => div()
            .group(group.clone())
            .child(copy(text, cx))
            .px_3()
            .py_2()
            .rounded_lg()
            .bg(danger.opacity(0.1))
            .small()
            .text_color(danger)
            .child(selectable(format!("agent-error-{agent}-{at}"), text))
            .into_any_element(),
    }
}

/// Who this is and how it stands, over its chat: the sun, the name and role, what it
/// does now and for how long, and a way to stop it or to put it away once it is done.
fn head(ws: &Workspace, agent: &str, cx: &mut Context<Workspace>) -> Option<Div> {
    let seen = ws.shown_chat()?.agents.get(agent)?;
    let theme = cx.theme();
    let muted = theme.muted_foreground;
    let color = match seen.state {
        AgentState::Done => theme.success.opacity(0.8),
        AgentState::Failed => theme.danger,
        _ => theme.warning,
    };
    let status = match &seen.state {
        AgentState::Thinking => "Thinking".to_owned(),
        AgentState::Writing => "Writing".to_owned(),
        AgentState::Tool(name) => dusk_protocol::tools::phrase(name, &seen.name).to_owned(),
        AgentState::Done => "Done".to_owned(),
        AgentState::Failed => "Failed".to_owned(),
    };
    let context = ws
        .shown_chat()
        .and_then(|chat| chat.agent_chats.get(agent))
        .and_then(|log| log.context_line());
    let id = agent.to_owned();
    let asked = ws
        .shown_chat()
        .is_some_and(|chat| chat.approval_waits_on(agent));
    let action = (!asked || seen.working()).then(|| {
        if seen.working() {
            Button::new("agent-chat-stop")
                .danger()
                .small()
                .label("Stop")
                .tooltip("Stop this agent; the others go on")
                .on_click(cx.listener(move |ws, _, _, cx| ws.stop_agent(&id, cx)))
        } else {
            Button::new("agent-chat-clear")
                .ghost()
                .small()
                .label("Clear")
                .tooltip("Take it off the list of subagents")
                .on_click(cx.listener(move |ws, _, _, cx| ws.clear_agents(Some(&id), cx)))
        }
    });
    Some(
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap_3()
            .pb_3()
            .border_b_1()
            .border_color(theme.border)
            .child(mascot::render(
                SharedString::from(format!("agent-chat-sun-{agent}")),
                &seen.state,
                color,
                2.0,
            ))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .small()
                            .text_color(muted)
                            .truncate()
                            .child(seen.role_and_model()),
                    )
                    .child(div().small().text_color(color).truncate().child(format!(
                        "{status} \u{00B7} {}",
                        dusk_protocol::thread::duration_words(
                            seen.run_ms(dusk_protocol::system::now_ms())
                        )
                    )))
                    .children(
                        context.map(|context| {
                            div().small().text_color(muted).truncate().child(context)
                        }),
                    ),
            )
            .children(action),
    )
}

impl Workspace {
    /// Opens a subagent's chat; its history is read once, then events keep it current.
    pub fn open_agent_chat(&mut self, agent: &str, cx: &mut Context<Self>) {
        let name = self
            .shown_chat()
            .and_then(|chat| chat.agents.get(agent))
            .map_or_else(|| agent.to_owned(), |agent| agent.name.clone());
        self.modal = Some(Modal::AgentChat {
            agent: agent.to_owned(),
            name,
        });
        // Opened again, it starts anew at its latest message.
        self.agent_list.agent = None;
        cx.notify();
        self.read_agent_contexts(Some(agent), cx);
        let known = self
            .shown_chat()
            .and_then(|chat| chat.agent_chats.get(agent))
            .is_some_and(|chat| !chat.items.is_empty());
        if known {
            return;
        }
        let (Some(parent), agent) = (self.shown.clone(), agent.to_owned()) else {
            return;
        };
        let asked = agent.clone();
        self.core.call(
            cx,
            move |client| async move { client.threads().messages(&asked).await },
            move |ws, result, cx| match result {
                Ok(history) => {
                    if let Some(chat) = ws.chats.get_mut(&parent) {
                        let live = chat.agent_chats.remove(&agent).unwrap_or_default();
                        let mut read = Log::from_history(&history);
                        // Events that came while the history loaded stay on top of it.
                        read.append_live(live);
                        chat.agent_chats.insert(agent.clone(), read);
                    }
                    cx.notify();
                }
                Err(error) => ws.fail(error, cx),
            },
        );
    }

    /// Stops one subagent; the chat's turn and the other agents go on.
    pub fn stop_agent(&mut self, agent: &str, cx: &mut Context<Self>) {
        let Some(parent) = self.shown.clone() else {
            return;
        };
        let (asked, stopped) = (agent.to_owned(), agent.to_owned());
        self.core.call(
            cx,
            move |client| async move { client.threads().stop_agent(&asked).await },
            move |ws, result, cx| match result {
                Ok(()) => {
                    // A stopped agent may end without another event; the view must not wait.
                    if let Some(seen) = ws
                        .chats
                        .get_mut(&parent)
                        .and_then(|chat| chat.agents.get_mut(&stopped))
                    {
                        seen.stop(dusk_protocol::system::now_ms());
                    }
                    cx.notify();
                }
                Err(error) => ws.fail(error, cx),
            },
        );
    }

    /// Takes one finished subagent off the list, or every finished one, for as long as the
    /// chat is open here; one an approval waits on stays. Its thread stays in the core.
    pub fn clear_agents(&mut self, only: Option<&str>, cx: &mut Context<Self>) {
        let Some(chat) = self.shown.clone().and_then(|id| self.chats.get_mut(&id)) else {
            return;
        };
        let gone = chat.clear_agents(only);
        // The chat of an agent just put away has nothing left to show.
        if let Some(Modal::AgentChat { agent, .. }) = &self.modal
            && gone.contains(agent)
        {
            self.modal = self
                .shown
                .clone()
                .map(|thread_id| Modal::Agents { thread_id });
        }
        cx.notify();
    }
}
