//! Completed questions and background commands keep their own shape in the transcript.

use crate::{ui::Type, workspace::Workspace};
use dusk_protocol::{thread::JobResult, tools::Answered};
use gpui_kit::{
    component::{ActiveTheme, IconName},
    prelude::FluentBuilder,
    *,
};

/// The answers to the agent's questions as one card: a row for each question, its answer
/// muted under it.
pub fn answered(key: &str, answers: &[Answered], cx: &App) -> Div {
    let theme = cx.theme();
    let mut card = div()
        .self_end()
        .w_full()
        .max_w(px(600.))
        .flex()
        .flex_col()
        .rounded_xl()
        .border_1()
        .border_color(theme.border)
        .bg(theme.secondary)
        .overflow_hidden();
    for (index, answer) in answers.iter().enumerate() {
        card = card.child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .px_4()
                .py_2()
                .when(index > 0, |row| row.border_t_1().border_color(theme.border))
                .child(super::super::links::markdown(
                    SharedString::from(format!("question-{key}-{index}")),
                    answer.question.clone(),
                    super::super::links::Links::Paths,
                ))
                .children(
                    answer
                        .lines()
                        .into_iter()
                        .map(|line| div().small().text_color(theme.muted_foreground).child(line)),
                ),
        );
    }
    card
}

pub fn job(
    ws: &Workspace,
    shown: &str,
    index: usize,
    result: &JobResult,
    cx: &mut Context<Workspace>,
) -> Div {
    let color = if result.failed() {
        cx.theme().danger
    } else {
        cx.theme().muted_foreground
    };
    let title = result.title();
    super::activity::activity_text(
        ws,
        shown,
        index,
        (
            &title,
            if result.failed() {
                IconName::CircleAlert
            } else {
                IconName::SquareTerminal
            },
        ),
        &result.output,
        || {
            div()
                .small()
                .text_color(color)
                .child(result.output.clone())
                .into_any_element()
        },
        cx,
    )
}
