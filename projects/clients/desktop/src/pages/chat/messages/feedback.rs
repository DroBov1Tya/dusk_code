//! Completed questions and background commands keep their own shape in the transcript.

use crate::{
    ui::{Type, widgets},
    workspace::Workspace,
};
use dusk_protocol::{thread::JobResult, tools::Answered};
use gpui_kit::{
    component::{ActiveTheme, IconName},
    prelude::FluentBuilder,
    *,
};

pub fn answered(key: &str, answers: &[Answered], cx: &App) -> Div {
    let theme = cx.theme();
    let mut body = div().flex().flex_col().gap_3();
    for (index, answer) in answers.iter().enumerate() {
        let mut question = div()
            .flex()
            .flex_col()
            .gap_1()
            .child(super::super::links::markdown(
                SharedString::from(format!("question-{key}-{index}")),
                answer.question.clone(),
            ));
        for (option, chosen) in &answer.options {
            question = question.child(
                div()
                    .flex()
                    .items_start()
                    .gap_2()
                    .small()
                    .text_color(if *chosen {
                        theme.foreground
                    } else {
                        theme.muted_foreground
                    })
                    .child(widgets::icon(
                        if *chosen {
                            IconName::Check
                        } else {
                            IconName::Minus
                        },
                        if *chosen {
                            theme.primary
                        } else {
                            theme.muted_foreground
                        },
                    ))
                    .child(div().flex_1().min_w_0().child(option.clone())),
            );
        }
        question = question.when(answer.declined, |row| {
            row.child(
                div()
                    .meta()
                    .text_color(theme.muted_foreground)
                    .child("Declined"),
            )
        });
        if !answer.own.is_empty() {
            question = question.child(super::super::links::markdown(
                SharedString::from(format!("answer-{key}-{index}")),
                answer.own.clone(),
            ));
        }
        body = body.child(question);
    }
    body
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
