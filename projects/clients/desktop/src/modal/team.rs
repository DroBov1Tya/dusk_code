//! Steering a running team: stopping it, after a yes, and changing its budget.

use dusk_protocol::thread::{BUDGET_FIELDS, Team, TeamBudget};
use gpui_kit::{
    component::{
        ActiveTheme,
        button::{Button, ButtonVariants},
        input::{Input, InputState},
    },
    *,
};

use super::Modal;
use crate::{ui::Type, workspace::Workspace};

/// The budget of a running team being changed.
pub struct BudgetForm {
    pub thread_id: String,
    pub fields: [Entity<InputState>; 4],
    /// Why the last save was refused, by this form or by the core.
    pub error: Option<String>,
}

pub fn stop(cx: &mut Context<Workspace>) -> Div {
    let muted = cx.theme().muted_foreground;
    div()
        .flex()
        .flex_col()
        .gap_3()
        .p_5()
        .child("Stop the team?")
        .child(div().small().text_color(muted).child(
            "Members at work are stopped and their tasks go back to open. The plan and \
             what is done stay; running the plan again starts a new team.",
        ))
        .child(
            div()
                .flex()
                .flex_row()
                .justify_end()
                .gap_2()
                .child(
                    Button::new("team-stop-cancel")
                        .ghost()
                        .label("Keep it running")
                        .on_click(cx.listener(|ws, _, _, cx| ws.close_modal(cx))),
                )
                .child(
                    Button::new("team-stop-confirm")
                        .danger()
                        .label("Stop team")
                        .on_click(cx.listener(|ws, _, _, cx| ws.stop_team(cx))),
                ),
        )
}

pub fn budget(form: &BudgetForm, cx: &mut Context<Workspace>) -> Div {
    let theme = cx.theme();
    let (muted, danger) = (theme.muted_foreground, theme.danger);
    let mut grid = div().grid().grid_cols(2).gap_3();
    for (name, field) in BUDGET_FIELDS.iter().zip(&form.fields) {
        grid = grid.child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(div().small().text_color(muted).child(*name))
                .child(Input::new(field)),
        );
    }
    div()
        .flex()
        .flex_col()
        .gap_3()
        .p_5()
        .child(div().small().text_color(muted).child(
            "What the team may spend from its start. The settings' ceilings still hold; \
             tokens may be written as 400k or 4M.",
        ))
        .child(grid)
        .children(
            form.error
                .clone()
                .map(|error| div().small().text_color(danger).child(error)),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .justify_end()
                .gap_2()
                .child(
                    Button::new("team-budget-cancel")
                        .ghost()
                        .label("Cancel")
                        .on_click(cx.listener(|ws, _, _, cx| ws.close_modal(cx))),
                )
                .child(
                    Button::new("team-budget-save")
                        .primary()
                        .label("Save budget")
                        .on_click(cx.listener(|ws, _, _, cx| ws.save_team_budget(cx))),
                ),
        )
}

impl Workspace {
    /// Asks before the shown chat's team is stopped.
    pub fn ask_stop_team(&mut self, cx: &mut Context<Self>) {
        if let Some(thread_id) = self.shown.clone() {
            self.modal = Some(Modal::StopTeam { thread_id });
            cx.notify();
        }
    }

    /// Stops the team; the core's events move its tasks and status.
    pub fn stop_team(&mut self, cx: &mut Context<Self>) {
        let Some(Modal::StopTeam { thread_id }) = self.modal.take() else {
            return;
        };
        cx.notify();
        self.core.call(
            cx,
            move |client| async move { client.work().stop_team(&thread_id).await },
            |ws, result, cx| match result {
                Ok(()) => ws.load_overview(cx),
                Err(error) => ws.fail(error, cx),
            },
        );
    }

    /// Opens the budget of `team` to change.
    pub fn ask_team_budget(&mut self, team: &Team, window: &mut Window, cx: &mut Context<Self>) {
        let fields = team.budget.fields().map(|text| {
            cx.new(|cx| {
                let mut state = InputState::new(window, cx);
                state.set_value(text, window, cx);
                state
            })
        });
        self.modal = Some(Modal::TeamBudget(BudgetForm {
            thread_id: team.thread_id.clone(),
            fields,
            error: None,
        }));
        cx.notify();
    }

    fn save_team_budget(&mut self, cx: &mut Context<Self>) {
        let Some(Modal::TeamBudget(form)) = &mut self.modal else {
            return;
        };
        let texts = form
            .fields
            .each_ref()
            .map(|field| field.read(cx).value().to_string());
        let read = TeamBudget::read(texts.each_ref().map(String::as_str));
        let budget = match read {
            Ok(budget) => budget,
            Err(error) => {
                form.error = Some(format!("{error:#}"));
                return cx.notify();
            }
        };
        let thread_id = form.thread_id.clone();
        self.core.call(
            cx,
            move |client| async move { client.work().set_team_budget(&thread_id, budget).await },
            |ws, result, cx| {
                match result {
                    Ok(()) => {
                        ws.modal = None;
                        ws.load_overview(cx);
                    }
                    // The core's ceilings are lower than the protocol's: it says which.
                    Err(error) => {
                        if let Some(Modal::TeamBudget(form)) = &mut ws.modal {
                            form.error = Some(format!("{error:#}"));
                        }
                    }
                }
                cx.notify();
            },
        );
    }
}
