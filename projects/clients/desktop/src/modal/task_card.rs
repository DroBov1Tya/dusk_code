//! A task of the plan as a card, opened from its row of the plan strip: what it is, why it
//! waits or failed, what it depends on and what it spent, and its member's chat a click
//! away. The fields are the protocol's (`task_card`); the window only lays them out.

use dusk_protocol::thread::task_card;
use gpui_kit::{
    component::{
        ActiveTheme,
        button::{Button, ButtonVariants},
    },
    *,
};

use super::Modal;
use crate::{
    ui::{Type, widgets},
    workspace::Workspace,
};

/// The card's title, or none once its task left the plan.
pub fn title(ws: &Workspace, task_id: &str) -> Option<String> {
    let chat = ws.shown_chat()?;
    task_card(&chat.tasks, task_id).map(|card| card.title)
}

pub fn render(ws: &Workspace, task_id: &str, cx: &mut Context<Workspace>) -> Div {
    let card = ws
        .shown_chat()
        .and_then(|chat| task_card(&chat.tasks, task_id));
    let muted = cx.theme().muted_foreground;
    let Some(card) = card else {
        return div()
            .p_5()
            .small()
            .text_color(muted)
            .child("This task is no longer in the plan.");
    };
    let color = widgets::tone(card.status.tone(), cx);
    let mut fields = div().flex().flex_col().gap_2();
    for (label, value) in &card.fields {
        fields = fields.child(
            div()
                .flex()
                .flex_row()
                .gap_3()
                .child(
                    div()
                        .flex_none()
                        .w(px(96.))
                        .meta()
                        .text_color(muted)
                        .child(*label),
                )
                .child(div().flex_1().min_w_0().small().child(value.clone())),
        );
    }
    let owner = card.owner.map(|owner| {
        let label = format!("Open {}'s chat", owner.name);
        Button::new("task-card-owner")
            .ghost()
            .label(label)
            .on_click(cx.listener(move |ws, _, _, cx| ws.open_agent_chat(&owner.thread_id, cx)))
    });
    div()
        .flex()
        .flex_col()
        .gap_4()
        .p_5()
        .child(div().small().text_color(color).child(card.head))
        .child(fields)
        .children(owner.map(|owner| div().flex().flex_row().justify_end().child(owner)))
}

impl Workspace {
    /// Opens the card of a task of the shown chat's plan.
    pub fn open_task_card(&mut self, task_id: &str, cx: &mut Context<Self>) {
        self.modal = Some(Modal::TaskCard {
            task_id: task_id.to_owned(),
        });
        cx.notify();
    }
}
