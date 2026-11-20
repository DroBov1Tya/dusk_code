//! The Plan layer: the tasks in waves, each with its owner, word and marks, the done
//! waves of a long plan folded into one column that a click unfolds.

use dusk_protocol::thread::{
    TaskStatus,
    team_layout::{Folded, Lit, Pick, Plan, Related, TaskNode},
};
use gpui_kit::{component::ActiveTheme, prelude::FluentBuilder, *};

use super::graph;
use crate::{
    ui::{Type, elevation, elevation::Level, widgets},
    workspace::Workspace,
};

pub fn render(layout: Plan, related: Option<&Related>, cx: &mut Context<Workspace>) -> Div {
    let muted = cx.theme().muted_foreground;
    let mut boxes = Vec::new();
    for head in &layout.heads {
        boxes.push(
            graph::at(head.rect)
                .flex()
                .flex_row()
                .gap_2()
                .meta()
                .text_color(muted)
                .child(head.name.clone())
                .child(div().opacity(0.75).child(head.done.clone()))
                .into_any_element(),
        );
    }
    if let Some(folded) = &layout.folded {
        boxes.push(fold(folded, cx).into_any_element());
    }
    for task in &layout.tasks {
        let lit = related.map(|related| related.task(&task.task_id));
        boxes.push(node(task, lit, cx).into_any_element());
    }
    let edges = graph::edges(layout.edges, related, cx);
    graph::layer(layout.width, layout.height, edges, boxes)
}

/// The done waves folded: how many tasks they hold; a click unfolds them.
fn fold(folded: &Folded, cx: &mut Context<Workspace>) -> Stateful<Div> {
    let theme = cx.theme();
    graph::at(folded.rect)
        .id("team-plan-fold")
        .rounded_lg()
        .border_1()
        .border_dashed()
        .border_color(theme.border)
        .cursor_pointer()
        .hover(|fold| fold.bg(cx.theme().list_hover))
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap_0p5()
        .meta()
        .text_color(theme.muted_foreground)
        .text_center()
        .child(folded.name.clone())
        .child(
            div()
                .body()
                .text_color(theme.success)
                .child(folded.tasks.to_string()),
        )
        .child("done")
        .on_click(cx.listener(|ws, _, _, cx| {
            ws.team_page.unfold = true;
            cx.notify();
        }))
}

/// A task: its glyph, number and words on two lines; under them its owner or role, its
/// word, how often it came back or was given, and whether a gate holds it.
fn node(task: &TaskNode, lit: Option<Lit>, cx: &mut Context<Workspace>) -> Stateful<Div> {
    let theme = cx.theme();
    let (muted, hover, accent) = (theme.muted_foreground, theme.list_hover, theme.primary);
    let color = widgets::tone(task.status.tone(), cx);
    let pick = Pick::Task(task.task_id.clone());
    let (over, open) = (pick.clone(), pick);
    let done = task.status == TaskStatus::Done;
    let owner = match &task.owner {
        Some((_, name)) => div()
            .flex_none()
            .px_1()
            .rounded_sm()
            .bg(theme.foreground.opacity(0.08))
            .text_color(theme.foreground.opacity(0.8))
            .child(name.clone()),
        None => div().flex_none().child(task.role.clone()),
    };
    let word = match task.for_user {
        true => {
            let task_id = task.task_id.clone();
            div()
                .id(SharedString::from(format!("team-confirm-{}", task.task_id)))
                .text_color(accent)
                .underline()
                .cursor_pointer()
                .child(task.word)
                .on_click(cx.listener(move |ws, _, _, cx| {
                    // The node under it opens the card; this click only confirms.
                    cx.stop_propagation();
                    ws.confirm_task(&task_id, cx)
                }))
                .into_any_element()
        }
        // Waiting in the queue says nothing beside the role.
        false if task.status == TaskStatus::Open && task.owner.is_none() => {
            div().into_any_element()
        }
        false => div()
            .text_color(match task.status {
                TaskStatus::Running | TaskStatus::Claimed => muted,
                _ => color,
            })
            .child(task.word)
            .into_any_element(),
    };
    let face = div()
        .id(SharedString::from(format!("team-task-{}", task.task_id)))
        .absolute()
        .left(px(task.rect.x))
        .top(px(task.rect.y))
        .w(px(task.rect.w))
        .h(px(task.rect.h))
        .rounded_lg()
        .border_1()
        .border_color(transparent_black())
        .bg(elevation::surface(Level::Raised, cx))
        .line_height(px(16.))
        .overflow_hidden()
        .cursor_pointer()
        .hover(move |node| node.bg(hover))
        .px_1p5()
        .pt_1()
        .flex()
        .flex_col()
        .on_hover(
            cx.listener(move |ws, inside: &bool, _, cx| ws.team_hover(over.clone(), *inside, cx)),
        )
        .on_click(cx.listener(move |ws, _, _, cx| ws.team_open(open.clone(), cx)));
    let face = graph::lit(face, lit, cx);
    face.child(
        div()
            .h(px(32.))
            .flex()
            .flex_row()
            .items_start()
            .gap_0p5()
            .child(graph::glyph(task.status, cx))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .meta()
                    .line_clamp(2)
                    .when(done, |title| title.text_color(muted))
                    .child(format!("#{} {}", task.number, task.title)),
            ),
    )
    .child(
        div()
            .h(px(18.))
            .mt_0p5()
            .pl(px(20.))
            .flex()
            .flex_row()
            .items_center()
            .gap_1()
            .text_size(rems(0.7))
            .text_color(muted)
            .child(owner)
            .child(div().flex_1().min_w_0().truncate().child(word))
            .when(task.reworks > 0, |row| {
                row.child(
                    div()
                        .flex_none()
                        .text_color(accent)
                        .child(format!("\u{21BA}{}", task.reworks)),
                )
            })
            // Come back from review says it was given again; the count would crowd it.
            .when(task.attempts > 1 && task.reworks == 0, |row| {
                row.child(
                    div()
                        .flex_none()
                        .child(format!("\u{00D7}{}", task.attempts)),
                )
            })
            .when(task.gated, |row| {
                let gate = match task.status {
                    TaskStatus::Review if task.for_user => theme.warning,
                    TaskStatus::Review => accent,
                    _ => muted,
                };
                row.child(gate_mark(gate))
            }),
    )
}

/// A gate: a small diamond on a line, as a command or a confirmation that must pass.
fn gate_mark(color: Hsla) -> impl IntoElement {
    div().flex_none().w(px(10.)).h(px(12.)).child(
        canvas(
            |_, _, _| {},
            move |bounds, _, window, _| {
                let at = |x: f32, y: f32| point(bounds.origin.x + px(x), bounds.origin.y + px(y));
                let strokes: [&[(f32, f32)]; 3] = [
                    &[(5., 0.), (5., 3.)],
                    &[(5., 9.), (5., 12.)],
                    &[(5., 3.), (8., 6.), (5., 9.), (2., 6.), (5., 3.)],
                ];
                for points in strokes {
                    let mut path = PathBuilder::stroke(px(1.2));
                    path.move_to(at(points[0].0, points[0].1));
                    for &(x, y) in &points[1..] {
                        path.line_to(at(x, y));
                    }
                    if let Ok(path) = path.build() {
                        window.paint_path(path, color);
                    }
                }
            },
        )
        .size_full(),
    )
}
