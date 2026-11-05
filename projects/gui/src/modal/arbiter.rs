//! The arbiter window: the chat's agents on the left, with who asked whom; the arbiter
//! in the middle as a sphere that stirs at each answer, with its tally; and on the
//! right what passed between them, newest first: tasks given, reports back, approval
//! requests and the arbiter's answers.

use gpui_kit::{
    component::{ActiveTheme, IconName},
    *,
};

use super::sphere;
use crate::{
    chat::transcript::{CHAT, ExchangeKind},
    widgets,
    workspace::Workspace,
};

/// How many exchanges the timeline shows.
const SHOWN: usize = 80;

pub fn render(ws: &Workspace, cx: &mut Context<Workspace>) -> Div {
    let theme = cx.theme();
    let (muted, border, accent, success, danger, warning, foreground) = (
        theme.muted_foreground,
        theme.border,
        theme.primary,
        theme.success,
        theme.danger,
        theme.warning,
        theme.foreground,
    );
    let Some(chat) = ws.shown_chat() else {
        return div()
            .p_5()
            .child(widgets::muted("Open a chat to see its arbiter.", cx));
    };
    let count = |kind: ExchangeKind| chat.exchanges.iter().filter(|e| e.kind == kind).count();
    let (asked, allowed, denied) = (
        count(ExchangeKind::Approval),
        count(ExchangeKind::Allowed),
        count(ExchangeKind::Denied),
    );
    // Left: the chat and the agents it asked.
    let mut agents = div()
        .flex()
        .flex_col()
        .gap_2()
        .child(widgets::caption("Agents", cx).px_0().pt_0())
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .child(widgets::icon(IconName::Inbox, accent))
                .child(div().font_weight(FontWeight::MEDIUM).child("The chat")),
        );
    for agent in chat.agents.values() {
        agents = agents.child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .pl_4()
                .border_l_1()
                .border_color(border)
                .child(div().size(px(6.)).rounded_full().bg(if agent.working {
                    accent
                } else {
                    muted.opacity(0.5)
                }))
                .child(div().text_sm().truncate().child(agent.name.clone())),
        );
    }
    if chat.agents.is_empty() {
        agents = agents.child(widgets::muted("No agents asked yet.", cx).px_0());
    }
    // Middle: the arbiter itself.
    let stat = |value: usize, label: &'static str, color: Hsla| {
        div()
            .flex()
            .flex_col()
            .items_center()
            .child(div().text_lg().text_color(color).child(value.to_string()))
            .child(div().text_xs().text_color(muted).child(label))
    };
    let middle = div()
        .flex()
        .flex_col()
        .items_center()
        .gap_3()
        .child(sphere::render(
            "arbiter-sphere",
            chat.arbiter_at,
            accent,
            warning,
        ))
        .child(
            div()
                .text_sm()
                .text_color(muted)
                .child(match ws.status.approvals {
                    dusk_protocol::config::ApprovalPolicy::Arbiter => {
                        "The arbiter answers the approvals"
                    }
                    _ => "The arbiter is off; approvals come to you",
                }),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .gap_6()
                .child(stat(asked, "asked", foreground))
                .child(stat(allowed, "allowed", success))
                .child(stat(denied, "denied", danger)),
        );
    // Right: the timeline.
    let mut timeline = div()
        .flex()
        .flex_col()
        .gap_2()
        .child(widgets::caption("What passed", cx).px_0().pt_0());
    if chat.exchanges.is_empty() {
        timeline = timeline.child(widgets::muted("Nothing yet.", cx).px_0());
    }
    for exchange in chat.exchanges.iter().rev().take(SHOWN) {
        let (label, color) = match exchange.kind {
            ExchangeKind::Asked => ("asks", accent),
            ExchangeKind::Reported => ("reports", foreground),
            ExchangeKind::Approval => ("needs a yes", warning),
            ExchangeKind::Allowed => ("allows", success),
            ExchangeKind::Denied => ("denies", danger),
        };
        let name = |who: &str| {
            if who == CHAT {
                "chat".to_owned()
            } else {
                who.to_owned()
            }
        };
        timeline = timeline.child(
            div()
                .flex()
                .flex_col()
                .gap_0p5()
                .pb_2()
                .border_b_1()
                .border_color(border)
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_1()
                        .text_xs()
                        .child(
                            div()
                                .font_weight(FontWeight::MEDIUM)
                                .child(name(&exchange.from)),
                        )
                        .child(div().text_color(color).child(format!("→ {label} →")))
                        .child(
                            div()
                                .font_weight(FontWeight::MEDIUM)
                                .child(name(&exchange.to)),
                        )
                        .child(div().flex_1())
                        .child(
                            div()
                                .text_color(muted)
                                .child(format!("{}s ago", exchange.at.elapsed().as_secs())),
                        ),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(muted)
                        .line_clamp(2)
                        .child(exchange.text.clone()),
                ),
        );
    }
    div()
        .flex()
        .flex_row()
        .gap_6()
        .p_5()
        .child(div().w(px(200.)).flex_none().child(agents))
        .child(div().flex_none().child(middle))
        .child(div().flex_1().min_w_0().child(timeline))
}
