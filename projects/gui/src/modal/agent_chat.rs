//! A subagent's chat, to read: what it was asked, what it said and the steps it took,
//! live while it works. It is the parent chat's to steer, so there is no composer.

use gpui_kit::{
    component::{
        ActiveTheme, IconName, Sizable,
        button::{Button, ButtonVariants},
    },
    *,
};

use super::Modal;
use crate::{
    columns::inspector::mascot,
    pages::chat::{
        links,
        messages::{activity_text, agent_report, run, steps},
        transcript::{AgentState, Item, Transcript},
    },
    ui::{Type, widgets},
    workspace::Workspace,
};

pub fn render(ws: &Workspace, agent: &str, cx: &mut Context<Workspace>) -> Div {
    let theme = cx.theme();
    let (muted, surface, accent, danger) = (
        theme.muted_foreground,
        theme.muted,
        theme.primary,
        theme.danger,
    );
    let mut column = div().flex().flex_col().gap_3().p_5();
    column = column.children(head(ws, agent, cx));
    let Some(chat) = ws.shown_chat().and_then(|chat| chat.agent_chats.get(agent)) else {
        return column.child(widgets::muted("Loading...", cx));
    };
    if chat.items.is_empty() {
        return column.child(widgets::muted("Nothing yet.", cx));
    }
    let mut index = 0;
    while index < chat.items.len() {
        let item = &chat.items[index];
        // The steps are folded as in the chat: the latest shows, the earlier ones unfold.
        if let Item::Tool(_) = item {
            let start = index;
            while index < chat.items.len() && matches!(chat.items[index], Item::Tool(_)) {
                index += 1;
            }
            let tools = steps(&chat.items[start..index]);
            if !tools.is_empty() {
                column = column.child(run(ws, agent, start, &tools, cx));
            }
            continue;
        }
        let at = index;
        index += 1;
        let key = |part: &str| SharedString::from(format!("agent-{part}-{agent}-{at}"));
        // Every message copies whole, from a button that comes up under the pointer.
        let group = key("row");
        let copy = |text: &str| {
            div()
                .flex()
                .justify_end()
                .child(widgets::copy(key("copy"), &group, text))
        };
        column = column.child(match item {
            // What it was asked reads as it was written: lists, code and all.
            Item::User { text, .. } => div()
                .group(group.clone())
                .px_3()
                .py_2()
                .rounded_lg()
                .bg(surface)
                .small()
                .child(links::markdown(key("asked"), text.clone()))
                .child(copy(text))
                .into_any_element(),
            Item::Agent { from, text } => {
                agent_report(ws, agent, at, from, text, cx).into_any_element()
            }
            Item::Assistant { text, reasoning } => {
                let mut said = div().flex().flex_col().gap_2();
                if !reasoning.trim().is_empty() {
                    let thought = reasoning.clone();
                    said = said.child(activity_text(
                        ws,
                        agent,
                        at,
                        ("Thinking", IconName::Asterisk),
                        reasoning,
                        move || thought.into_any_element(),
                        cx,
                    ));
                }
                if !text.is_empty() {
                    said = said
                        .group(group.clone())
                        .child(links::markdown(key("said"), text.clone()))
                        .child(copy(text));
                }
                said.into_any_element()
            }
            Item::Tool(_) => div().into_any_element(),
            Item::Command(run) => div()
                .text_xs()
                .text_color(muted)
                .child(format!("$ {}", run.command))
                .into_any_element(),
            Item::Audio(clip) => div()
                .text_xs()
                .text_color(muted)
                .child(clip.title.clone().unwrap_or_else(|| "A sound".to_owned()))
                .into_any_element(),
            Item::Image(picture) => div()
                .text_xs()
                .text_color(muted)
                .child(
                    picture
                        .title
                        .clone()
                        .unwrap_or_else(|| "A picture".to_owned()),
                )
                .into_any_element(),
            Item::Notice(text) => div()
                .text_xs()
                .text_color(muted)
                .child(text.clone())
                .into_any_element(),
            // What went wrong must not read as one more quiet note.
            Item::Error(text) => div()
                .group(group.clone())
                .child(copy(text))
                .px_3()
                .py_2()
                .rounded_lg()
                .bg(danger.opacity(0.1))
                .small()
                .text_color(danger)
                .child(text.clone())
                .into_any_element(),
        });
    }
    if chat.working {
        column = column.child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_3()
                .meta()
                .text_color(muted)
                .child(crate::pages::chat::working_mark(
                    SharedString::from(format!("agent-working-{agent}")),
                    accent,
                    cx.theme().foreground,
                ))
                .child("Working"),
        );
    }
    column
}

/// Who this is and how it stands, over its chat: the fox, the name and role, what it
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
    let id = agent.to_owned();
    let action = if seen.working {
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
    };
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
                SharedString::from(format!("agent-chat-fox-{agent}")),
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
                            .child(seen.role.clone()),
                    )
                    .child(div().small().text_color(color).truncate().child(format!(
                        "{status} \u{00B7} {}",
                        dusk_protocol::thread::duration_words(seen.run_time().as_millis() as i64)
                    ))),
            )
            .child(action),
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
        cx.notify();
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
                        let mut read = Transcript::from_history(&history);
                        // Events that came while the history loaded stay on top of it.
                        read.items.extend(live.items);
                        read.working = live.working;
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
                        seen.stop();
                    }
                    cx.notify();
                }
                Err(error) => ws.fail(error, cx),
            },
        );
    }

    /// Takes one finished subagent off the list, or every finished one. Its thread stays
    /// in the core; only this window stops showing it.
    pub fn clear_agents(&mut self, only: Option<&str>, cx: &mut Context<Self>) {
        let Some(chat) = self.shown.clone().and_then(|id| self.chats.get_mut(&id)) else {
            return;
        };
        let gone: Vec<String> = chat
            .agents
            .iter()
            .filter(|(id, agent)| !agent.working && only.is_none_or(|only| only == id.as_str()))
            .map(|(id, _)| id.clone())
            .collect();
        for id in &gone {
            chat.agents.remove(id);
            chat.agent_chats.remove(id);
        }
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
