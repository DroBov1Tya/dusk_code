//! A subagent's chat, to read: what it was asked, what it said and the steps it took,
//! live while it works. It is the parent chat's to steer, so there is no composer.

use gpui_kit::{
    component::{ActiveTheme, IconName},
    *,
};

use super::Modal;
use crate::{
    pages::chat::{
        links,
        transcript::{Item, ToolState, Transcript},
    },
    ui::widgets,
    workspace::Workspace,
};

pub fn render(ws: &Workspace, agent: &str, cx: &mut Context<Workspace>) -> Div {
    let theme = cx.theme();
    let (muted, surface, success, danger, accent) = (
        theme.muted_foreground,
        theme.muted,
        theme.success,
        theme.danger,
        theme.primary,
    );
    let mut column = div().flex().flex_col().gap_3().p_5();
    let Some(chat) = ws.shown_chat().and_then(|chat| chat.agent_chats.get(agent)) else {
        return column.child(widgets::muted("Loading...", cx));
    };
    if chat.items.is_empty() {
        return column.child(widgets::muted("Nothing yet.", cx));
    }
    for (index, item) in chat.items.iter().enumerate() {
        column = column.child(match item {
            Item::User { text, .. } => div()
                .px_3()
                .py_2()
                .rounded_lg()
                .bg(surface)
                .text_sm()
                .child(text.clone())
                .into_any_element(),
            Item::Agent { from, text } => div()
                .text_xs()
                .text_color(muted)
                .child(format!("{from} reported: {text}"))
                .into_any_element(),
            Item::Assistant { text, .. } if !text.is_empty() => links::markdown(
                SharedString::from(format!("agent-{agent}-{index}")),
                text.clone(),
            )
            .into_any_element(),
            Item::Assistant { .. } => div().into_any_element(),
            Item::Tool(tool) => {
                let (icon, color) = match tool.state {
                    ToolState::Running => (IconName::LoaderCircle, accent),
                    ToolState::Done => (IconName::Check, success),
                    ToolState::Failed => (IconName::CircleX, danger),
                };
                div()
                    .flex()
                    .flex_row()
                    .gap_2()
                    .items_center()
                    .text_xs()
                    .text_color(muted)
                    .child(widgets::icon(icon, color))
                    .child(tool.name.clone())
                    .child(div().truncate().child(tool.detail.clone()))
                    .into_any_element()
            }
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
            Item::Notice(text) | Item::Error(text) => div()
                .text_xs()
                .text_color(muted)
                .child(text.clone())
                .into_any_element(),
        });
    }
    if chat.working {
        column = column.child(div().text_xs().text_color(accent).child("Working"));
    }
    column
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
}
