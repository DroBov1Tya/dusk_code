//! The People layer: the lead, its members as nodes, the done ones as a line or folded
//! into chips, and the edges between them with their words.

use dusk_protocol::thread::{
    Team, Work, WorkKind,
    team_layout::{EdgeTone, FoldChip, MemberNode, People, Pick, Related},
};
use gpui_kit::{component::ActiveTheme, prelude::FluentBuilder, *};

use super::graph;
use crate::{
    pages::chat::activity,
    ui::{Type, elevation, elevation::Level},
    workspace::Workspace,
};

pub fn render(
    layout: People,
    team: &Team,
    related: Option<&Related>,
    cx: &mut Context<Workspace>,
) -> Div {
    let mut boxes = vec![lead(&layout, team, cx).into_any_element()];
    for label in &layout.labels {
        let lit = related.is_none_or(|related| related.edge(&label.ends));
        let color = label_color(label.tone, cx);
        boxes.push(
            graph::at(label.rect)
                .meta()
                .text_color(if lit { color } else { color.opacity(0.38) })
                .truncate()
                .when(label.right, |label| label.text_right())
                .child(label.text.clone())
                .into_any_element(),
        );
    }
    for member in &layout.members {
        let lit = related.map(|related| related.member(&member.thread_id));
        boxes.push(node(member, lit, cx).into_any_element());
    }
    for (index, chip) in layout.folds.iter().enumerate() {
        boxes.push(fold(index, chip, cx).into_any_element());
    }
    let edges = graph::edges(layout.edges, related, cx);
    graph::layer(layout.width, layout.height, edges, boxes)
}

fn label_color(tone: EdgeTone, cx: &App) -> Hsla {
    let theme = cx.theme();
    match tone {
        EdgeTone::Accent => theme.primary,
        EdgeTone::Warning => theme.warning,
        EdgeTone::Danger => theme.danger,
        EdgeTone::Line | EdgeTone::Faint => theme.muted_foreground,
    }
}

/// The lead: this chat, how its team stands and how far the plan is.
fn lead(layout: &People, team: &Team, cx: &App) -> Div {
    let theme = cx.theme();
    let muted = theme.muted_foreground;
    let mark = match team.ended() {
        None => activity::mark(
            &Work {
                kind: WorkKind::Agents {
                    working: team.at_work() as u32,
                    started: team.members.len() as u32,
                },
                label: String::new(),
                stalled: false,
            },
            cx,
        ),
        Some(dusk_protocol::thread::TeamEnd::Done) => {
            graph::glyph(dusk_protocol::thread::TaskStatus::Done, cx)
        }
        Some(_) => graph::glyph(dusk_protocol::thread::TaskStatus::Blocked, cx),
    };
    graph::at(layout.lead)
        .rounded_lg()
        .bg(elevation::surface(Level::Raised, cx))
        .line_height(px(18.))
        .overflow_hidden()
        .px_2()
        .pt(px(6.))
        .flex()
        .flex_col()
        .child(
            div()
                .h(px(18.))
                .flex()
                .flex_row()
                .items_center()
                .gap_1()
                .child(mark)
                .child(div().small().font_weight(FontWeight::MEDIUM).child("Lead"))
                .child(div().meta().text_color(muted).child("this chat")),
        )
        .child(
            div()
                .pl(px(24.))
                .meta()
                .text_color(theme.foreground.opacity(0.8))
                .truncate()
                .child(layout.lead_line.clone()),
        )
        .child(
            div()
                .pl(px(24.))
                .meta()
                .text_color(muted)
                .truncate()
                .child(layout.lead_tasks.clone()),
        )
}

/// A member: its mark, name, role and model, what it does, and its task with what it
/// spent; a done one in a line. The pointer on it lights its task below; a click opens
/// its chat.
fn node(
    member: &MemberNode,
    lit: Option<dusk_protocol::thread::team_layout::Lit>,
    cx: &mut Context<Workspace>,
) -> Stateful<Div> {
    let theme = cx.theme();
    let (muted, hover) = (theme.muted_foreground, theme.list_hover);
    let pick = Pick::Member(member.thread_id.clone());
    let (over, open) = (pick.clone(), pick);
    let face = div()
        .id(SharedString::from(format!(
            "team-member-{}",
            member.thread_id
        )))
        .absolute()
        .left(px(member.rect.x))
        .top(px(member.rect.y))
        .w(px(member.rect.w))
        .h(px(member.rect.h))
        .rounded_lg()
        .border_1()
        .border_color(transparent_black())
        .bg(elevation::surface(Level::Raised, cx))
        .line_height(px(18.))
        .overflow_hidden()
        .cursor_pointer()
        .hover(move |node| node.bg(hover))
        .on_hover(
            cx.listener(move |ws, inside: &bool, _, cx| ws.team_hover(over.clone(), *inside, cx)),
        )
        .on_click(cx.listener(move |ws, _, _, cx| ws.team_open(open.clone(), cx)));
    let face = graph::lit(face, lit, cx);
    let spend = member
        .spend
        .clone()
        .map(|spend| div().flex_none().meta().text_color(muted).child(spend));
    if member.one_line {
        return face
            .px_2()
            .flex()
            .flex_row()
            .items_center()
            .gap_1()
            .child(graph::member_mark(&member.state, cx))
            .child(
                div()
                    .flex_none()
                    .small()
                    .text_color(muted)
                    .child(member.name.clone()),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .meta()
                    .text_color(muted)
                    .truncate()
                    .child(member.task.clone().unwrap_or_default()),
            )
            .children(spend);
    }
    let about = [Some(member.role.clone()), member.model.clone()]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" \u{00B7} ");
    let line_color = match member.state.status() {
        dusk_protocol::thread::TaskStatus::Blocked => theme.warning,
        dusk_protocol::thread::TaskStatus::Failed => theme.danger,
        _ => theme.foreground.opacity(0.8),
    };
    face.px_2()
        .pt(px(6.))
        .flex()
        .flex_col()
        .child(
            div()
                .h(px(18.))
                .flex()
                .flex_row()
                .items_center()
                .gap_1()
                .child(graph::member_mark(&member.state, cx))
                .child(
                    div()
                        .flex_none()
                        .small()
                        .font_weight(FontWeight::MEDIUM)
                        .child(member.name.clone()),
                )
                .child(
                    div()
                        .min_w_0()
                        .meta()
                        .text_color(muted)
                        .truncate()
                        .child(about),
                ),
        )
        .child(
            div()
                .h(px(18.))
                .pl(px(24.))
                .meta()
                .text_color(line_color)
                .truncate()
                .child(member.line.clone()),
        )
        .child(
            div()
                .h(px(18.))
                .pl(px(24.))
                .flex()
                .flex_row()
                .gap_2()
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .meta()
                        .text_color(muted)
                        .truncate()
                        .child(member.task.clone().unwrap_or_default()),
                )
                .children(spend),
        )
}

/// Members folded into one chip; who is in it shows on hover.
fn fold(index: usize, chip: &FoldChip, cx: &App) -> Stateful<Div> {
    let theme = cx.theme();
    let names = chip.names.join("\n");
    graph::at(chip.rect)
        .id(("team-fold", index))
        .rounded_lg()
        .border_1()
        .border_dashed()
        .border_color(theme.border)
        .px_2()
        .flex()
        .flex_row()
        .items_center()
        .gap_1()
        .child(graph::glyph(chip.status, cx))
        .child(
            div()
                .min_w_0()
                .meta()
                .text_color(theme.muted_foreground)
                .truncate()
                .child(chip.text.clone()),
        )
        .tooltip(move |window, cx| {
            gpui_kit::component::tooltip::Tooltip::new(names.clone()).build(window, cx)
        })
}
