//! The evaluator's verdict on the chat's goal after a turn: one line that says how it
//! stands (not met yet, achieved, failed), folded over the reason it gave.

use gpui_kit::{component::ActiveTheme, prelude::FluentBuilder, *};

use dusk_protocol::thread::{GoalEvaluation, GoalVerdict};

use super::activity::{disclosure, disclosure_icon, selectable, step_muted};
use crate::{
    ui::{Type, widgets},
    workspace::Workspace,
};

pub fn verdict(
    ws: &Workspace,
    shown: &str,
    index: usize,
    evaluation: &GoalEvaluation,
    cx: &mut Context<Workspace>,
) -> Div {
    let open = ws.unfolded.contains(&(shown.to_owned(), index));
    let theme = cx.theme();
    let muted = step_muted(cx);
    // Only an end of the goal takes a colour; a turn not met yet reads as a step.
    let (mark, tint) = match evaluation.verdict {
        GoalVerdict::NotMet => (None, muted),
        GoalVerdict::Met => (
            Some(gpui_kit::component::IconName::CircleCheck),
            theme.success,
        ),
        GoalVerdict::Impossible => (Some(gpui_kit::component::IconName::CircleX), theme.danger),
    };
    let border = theme.border;
    let title = evaluation.title();
    let ended = mark.is_some();
    let id = shown.to_owned();
    div()
        .flex()
        .flex_col()
        .gap_1()
        .child(
            disclosure(
                SharedString::from(format!("goal-{shown}-{index}")),
                open,
                cx,
            )
            .aria_label(title.clone())
            .child(match mark {
                Some(icon) => widgets::icon(icon, tint).into_any_element(),
                None => div()
                    .flex_none()
                    .w(px(crate::ui::size::ICON))
                    .flex()
                    .justify_center()
                    .text_color(tint)
                    .child("\u{25CE}")
                    .into_any_element(),
            })
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .when(ended, |title| title.text_color(tint))
                    .child(title),
            )
            .child(disclosure_icon(open, muted))
            .on_click(cx.listener(move |ws, _, _, cx| ws.fold(&id, index, cx))),
        )
        .when(open, |row| {
            row.child(
                div()
                    .ml_4()
                    .pl_3()
                    .border_l_1()
                    .border_color(border)
                    .small()
                    .text_color(muted)
                    .child(selectable(
                        format!("goal-reason-{shown}-{index}"),
                        &evaluation.reason,
                    )),
            )
        })
}
