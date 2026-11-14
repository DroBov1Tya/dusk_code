//! The arbiter window: the chat's agents on the left, with who asked whom; the arbiter
//! in the middle as a sphere that stirs at each answer, with its tally; and on the
//! right what passed between them, newest first: tasks given, reports back, approval
//! requests and the arbiter's answers.

use std::time::Duration;

use gpui_kit::{
    component::{ActiveTheme, IconName},
    prelude::FluentBuilder,
    *,
};

use super::sphere;
use crate::{
    pages::chat::transcript::{CHAT, ExchangeKind},
    ui::widgets,
    workspace::Workspace,
};

/// How many exchanges the timeline shows.
const SHOWN: usize = 80;

pub fn render(ws: &Workspace, thread_id: &str, cx: &mut Context<Workspace>) -> Div {
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
    let Some(chat) = ws.chats.get(thread_id) else {
        return div()
            .p_5()
            .child(widgets::muted("This chat is closed.", cx));
    };
    let count = |kind: ExchangeKind| chat.exchanges.iter().filter(|e| e.kind == kind).count();
    let (asked, allowed, denied) = (
        count(ExchangeKind::Approval),
        count(ExchangeKind::Allowed),
        count(ExchangeKind::Denied),
    );
    // Left: the chat and the agents it asked, as a tree, each with how much it said.
    let said = |name: &str| {
        chat.exchanges
            .iter()
            .filter(|exchange| exchange.from == name || exchange.to == name)
            .count()
    };
    let mut agents = div()
        .flex()
        .flex_col()
        .gap_1()
        .child(widgets::caption("Agents", cx).px_0().pt_0())
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .h(px(32.))
                .child(widgets::icon(IconName::Inbox, accent))
                .child(div().font_weight(FontWeight::MEDIUM).child("The chat")),
        );
    let mut branch = div()
        .flex()
        .flex_col()
        .ml(px(7.))
        .pl_3()
        .border_l_1()
        .border_color(border);
    for agent in chat.agents.values() {
        let dot = div().size(px(6.)).flex_none().rounded_full();
        // The one at work breathes; the others rest.
        let dot = if agent.working {
            dot.bg(accent)
                .with_animation(
                    SharedString::from(format!("arbiter-agent-{}", agent.name)),
                    Animation::new(Duration::from_millis(1400)).repeat(),
                    |dot, at| dot.opacity(0.35 + 0.65 * (at * std::f32::consts::TAU).cos().abs()),
                )
                .into_any_element()
        } else {
            dot.bg(muted.opacity(0.45)).into_any_element()
        };
        branch = branch.child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .min_h(px(40.))
                .child(dot)
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .child(div().text_sm().truncate().child(agent.name.clone()))
                        .child(
                            div()
                                .text_xs()
                                .text_color(muted)
                                .truncate()
                                .child(agent.role.clone()),
                        ),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(muted)
                        .child(said(&agent.name).to_string()),
                ),
        );
    }
    agents = if chat.agents.is_empty() {
        agents.child(widgets::muted("No agents asked yet.", cx).px_0())
    } else {
        agents.child(branch)
    };
    // Middle: the arbiter itself, how it stands, and its tally on one plate.
    let on = matches!(
        chat.access.unwrap_or(ws.status.approvals),
        dusk_protocol::config::ApprovalPolicy::Arbiter
    );
    let stat = |value: usize, label: &'static str, color: Hsla| {
        div()
            .flex_1()
            .flex()
            .flex_col()
            .items_center()
            .py_2()
            .child(
                div()
                    .text_lg()
                    .font_weight(FontWeight::MEDIUM)
                    // A count of nothing says nothing: it waits in the quiet color.
                    .text_color(if value == 0 {
                        muted.opacity(0.6)
                    } else {
                        color
                    })
                    .child(value.to_string()),
            )
            .child(div().text_xs().text_color(muted).child(label))
    };
    let state = if on { success } else { muted };
    let middle = div()
        .flex()
        .flex_col()
        .items_center()
        .gap_4()
        .child(sphere::render(
            SharedString::from(format!("arbiter-sphere-{thread_id}")),
            chat.arbiter_at,
            accent,
            warning,
        ))
        .child(
            div()
                .flex()
                .flex_col()
                .items_center()
                .gap_1()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .px_3()
                        .h(px(26.))
                        .rounded_full()
                        .bg(state.opacity(0.12))
                        .text_xs()
                        .text_color(state)
                        .child(div().size(px(6.)).rounded_full().bg(state))
                        .child(if on { "Arbiter on" } else { "Arbiter off" }),
                )
                .child(div().text_xs().text_color(muted).child(if on {
                    "It answers the approvals in your place"
                } else {
                    "Approvals come to you"
                })),
        )
        .child(
            div()
                .w(px(232.))
                .flex()
                .flex_row()
                .rounded_lg()
                .bg(theme.muted)
                .child(stat(asked, "asked", foreground))
                .child(div().w(px(1.)).my_2().bg(border))
                .child(stat(allowed, "allowed", success))
                .child(div().w(px(1.)).my_2().bg(border))
                .child(stat(denied, "denied", danger)),
        );
    // Right: what passed, newest first, each on a line down the side with a mark that
    // says what kind of thing it was.
    let mut timeline = div()
        .flex()
        .flex_col()
        .child(widgets::caption("What passed", cx).px_0().pt_0().pb_2());
    if chat.exchanges.is_empty() {
        timeline = timeline.child(widgets::muted("Nothing yet.", cx).px_0());
    }
    let name = |who: &str| {
        if who == CHAT {
            "the chat".to_owned()
        } else {
            who.to_owned()
        }
    };
    let shown = chat.exchanges.len().min(SHOWN);
    for (index, exchange) in chat.exchanges.iter().rev().take(SHOWN).enumerate() {
        let (icon, color, line) = match exchange.kind {
            ExchangeKind::Asked => (
                IconName::ChevronRight,
                accent,
                format!("{} asked {}", name(&exchange.from), name(&exchange.to)),
            ),
            ExchangeKind::Reported => (
                IconName::Check,
                foreground,
                format!(
                    "{} reported to {}",
                    name(&exchange.from),
                    name(&exchange.to)
                ),
            ),
            ExchangeKind::Approval => (
                IconName::Eye,
                warning,
                format!("{} needs a yes", name(&exchange.from)),
            ),
            ExchangeKind::Allowed => (
                IconName::CircleCheck,
                success,
                format!("{} allowed {}", name(&exchange.from), name(&exchange.to)),
            ),
            ExchangeKind::Denied => (
                IconName::Close,
                danger,
                format!("{} denied {}", name(&exchange.from), name(&exchange.to)),
            ),
        };
        let ago = dusk_protocol::thread::duration_words(exchange.at.elapsed().as_millis() as i64);
        timeline = timeline.child(
            div()
                .flex()
                .flex_row()
                .gap_3()
                .child(
                    div()
                        .flex_none()
                        .w(px(24.))
                        .flex()
                        .flex_col()
                        .items_center()
                        .child(
                            div()
                                .size(px(24.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded_full()
                                .bg(color.opacity(0.14))
                                .child(widgets::icon(icon, color).size(px(13.))),
                        )
                        .when(index + 1 < shown, |rail| {
                            rail.child(div().flex_1().min_h(px(10.)).w(px(1.)).my_1().bg(border))
                        }),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap_0p5()
                        .pb_4()
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_baseline()
                                .gap_2()
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .truncate()
                                        .text_sm()
                                        .font_weight(FontWeight::MEDIUM)
                                        .child(line),
                                )
                                .child(
                                    div()
                                        .flex_none()
                                        .text_xs()
                                        .text_color(muted)
                                        .child(format!("{ago} ago")),
                                ),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(muted)
                                .line_clamp(2)
                                .child(plain(&exchange.text)),
                        ),
                ),
        );
    }
    div()
        .flex()
        .flex_row()
        .gap_8()
        .p_6()
        .child(div().w(px(210.)).flex_none().child(agents))
        .child(div().flex_none().child(middle))
        .child(div().flex_1().min_w_0().child(timeline))
}

/// What was said as a line or two of plain words: a report is markdown over many
/// lines, and here it is only told apart from its neighbors.
fn plain(text: &str) -> String {
    let words: Vec<&str> = text
        .lines()
        .map(|line| line.trim().trim_start_matches(['#', '-', '*', '>', ' ']))
        .filter(|line| !line.is_empty())
        .collect();
    words
        .join(" \u{00B7} ")
        .replace(['*', '`'], "")
        .chars()
        .take(260)
        .collect()
}
