//! The chat's own access mode, over the bar: what its tools may do without asking. The
//! setting in Safety is the default; a choice here holds for this chat only.

use dusk_protocol::config::ApprovalPolicy;
use gpui_kit::{
    component::{ActiveTheme, IconName},
    prelude::FluentBuilder,
    *,
};

use super::model_bar::{picker_heading, picker_hint, picker_panel};
use crate::{
    ui::{Type, widgets},
    workspace::Workspace,
};

const CHOICES: [(Option<ApprovalPolicy>, &str, &str); 5] = [
    (None, "Default", "As set in Safety"),
    (
        Some(ApprovalPolicy::Ask),
        "Ask",
        "Asks before a command or a write outside",
    ),
    (
        Some(ApprovalPolicy::Sandbox),
        "Sandbox",
        "Commands run confined to the project",
    ),
    (
        Some(ApprovalPolicy::Arbiter),
        "Arbiter",
        "A second model answers the approvals",
    ),
    (
        Some(ApprovalPolicy::FullAccess),
        "Full access",
        "No questions; only for trusted work",
    ),
];

/// The mode the shown chat works in, its own or the default.
pub fn current(ws: &Workspace) -> ApprovalPolicy {
    ws.shown_chat()
        .and_then(|chat| chat.access)
        .unwrap_or(ws.status.approvals)
}

pub fn render(ws: &Workspace, cx: &mut Context<Workspace>) -> Stateful<Div> {
    let theme = cx.theme();
    let (muted, accent) = (picker_hint(cx), theme.primary);
    let own = ws.shown_chat().and_then(|chat| chat.access);
    let mut list = picker_panel(cx)
        .id("access-menu")
        .absolute()
        .occlude()
        .bottom(px(36.))
        .left_0()
        .w(px(336.))
        .max_h(px(400.))
        .overflow_y_scroll()
        .child(picker_heading("Permissions", cx));
    for (policy, name, hint) in CHOICES {
        let label = match policy {
            None => format!("{name} ({})", ws.status.approvals.name()),
            Some(_) => name.to_owned(),
        };
        list = list.child(
            widgets::row(
                SharedString::from(format!("access-{name}")),
                own == policy,
                cx,
            )
            .h_auto()
            .items_start()
            .min_h(px(48.))
            .px_3()
            .py_2()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .gap_0p5()
                    .child(div().body().font_weight(FontWeight::MEDIUM).child(label))
                    .child(div().small().text_color(muted).child(hint)),
            )
            .when(own == policy, |row| {
                row.child(widgets::icon(IconName::Check, accent))
            })
            .on_click(cx.listener(move |ws, _, _, cx| ws.choose_access(policy, cx))),
        );
    }
    list.on_mouse_down_out(cx.listener(|ws, _, _, cx| {
        ws.picker = None;
        ws.picker_closed = Some(std::time::Instant::now());
        cx.notify();
    }))
}

impl Workspace {
    fn choose_access(&mut self, policy: Option<ApprovalPolicy>, cx: &mut Context<Self>) {
        self.picker = None;
        let Some(thread_id) = self.shown.clone() else {
            cx.notify();
            return;
        };
        let id = thread_id.clone();
        self.core.call(
            cx,
            move |client| async move { client.threads().set_approvals(&id, policy).await },
            move |ws, result, cx| match result {
                Ok(()) => {
                    if let Some(chat) = ws.chats.get_mut(&thread_id) {
                        chat.access = policy;
                    }
                    cx.notify();
                }
                Err(error) => ws.fail(error, cx),
            },
        );
        cx.notify();
    }
}
