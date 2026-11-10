//! The chat's own access mode, over the bar: what its tools may do without asking. The
//! setting in Safety is the default; a choice here holds for this chat only.

use dusk_protocol::config::ApprovalPolicy;
use gpui_kit::{
    component::{ActiveTheme, IconName},
    prelude::FluentBuilder,
    *,
};

use crate::{widgets, workspace::Workspace};

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
    let (muted, accent) = (theme.muted_foreground, theme.primary);
    let own = ws.shown_chat().and_then(|chat| chat.access);
    let mut list = div()
        .id("access-menu")
        .absolute()
        .bottom(px(40.))
        .left(px(240.))
        .w(px(320.))
        .p_1()
        .flex()
        .flex_col()
        .rounded_xl()
        .border_1()
        .border_color(theme.border)
        .bg(theme.popover)
        .shadow_lg();
    for (policy, name, hint) in CHOICES {
        let label = match policy {
            None => format!("{name} ({})", ws.status.approvals.name()),
            Some(_) => name.to_owned(),
        };
        list = list.child(
            div()
                .id(SharedString::from(format!("access-{name}")))
                .flex()
                .flex_row()
                .gap_2()
                .px_2()
                .py_2()
                .rounded_md()
                .cursor_pointer()
                .hover(|row| row.bg(theme.accent))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .gap_0p5()
                        .child(div().font_weight(FontWeight::MEDIUM).child(label))
                        .child(div().text_xs().text_color(muted).child(hint)),
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
