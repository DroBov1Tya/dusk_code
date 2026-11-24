//! Steering a running team: stopping it, after a yes, and changing how many members work
//! at once.

use dusk_protocol::thread::{MAX_TEAM_AT_ONCE, Team};
use gpui_kit::{
    component::{
        ActiveTheme,
        button::{Button, ButtonVariants},
    },
    *,
};

use super::Modal;
use crate::{
    ui::{Type, controls},
    workspace::Workspace,
};

/// How many members of a running team work at once, being changed.
pub struct AtOnceForm {
    pub thread_id: String,
    pub at_once: u32,
    /// Why the last save was refused by the core.
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

pub fn at_once(form: &AtOnceForm, cx: &mut Context<Workspace>) -> Div {
    let theme = cx.theme();
    let (muted, danger) = (theme.muted_foreground, theme.danger);
    let at_once = u64::from(form.at_once);
    let stepper = controls::stepper(
        "team-at-once",
        at_once,
        (1, u64::from(MAX_TEAM_AT_ONCE)),
        at_once.to_string(),
        cx.listener(|ws, _, _, cx| ws.step_team_at_once(-1, cx)),
        cx.listener(|ws, _, _, cx| ws.step_team_at_once(1, cx)),
        cx,
    );
    div()
        .flex()
        .flex_col()
        .gap_3()
        .p_5()
        .child(div().small().text_color(muted).child(
            "Members of the team at work at once. Members already at work go on; fewer \
             takes hold as they finish. The settings' team_at_once is the most.",
        ))
        .child(div().flex().flex_row().child(stepper))
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
                    Button::new("team-at-once-cancel")
                        .ghost()
                        .label("Cancel")
                        .on_click(cx.listener(|ws, _, _, cx| ws.close_modal(cx))),
                )
                .child(
                    Button::new("team-at-once-save")
                        .primary()
                        .label("Save")
                        .on_click(cx.listener(|ws, _, _, cx| ws.save_team_at_once(cx))),
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

    /// Opens how many members of `team` work at once, to change.
    pub fn ask_team_at_once(&mut self, team: &Team, cx: &mut Context<Self>) {
        self.modal = Some(Modal::TeamAtOnce(AtOnceForm {
            thread_id: team.thread_id.clone(),
            at_once: team.at_once,
            error: None,
        }));
        cx.notify();
    }

    fn step_team_at_once(&mut self, by: i32, cx: &mut Context<Self>) {
        let Some(Modal::TeamAtOnce(form)) = &mut self.modal else {
            return;
        };
        form.at_once = form
            .at_once
            .saturating_add_signed(by)
            .clamp(1, MAX_TEAM_AT_ONCE);
        form.error = None;
        cx.notify();
    }

    fn save_team_at_once(&mut self, cx: &mut Context<Self>) {
        let Some(Modal::TeamAtOnce(form)) = &self.modal else {
            return;
        };
        let (thread_id, at_once) = (form.thread_id.clone(), form.at_once);
        self.core.call(
            cx,
            move |client| async move { client.work().set_team_at_once(&thread_id, at_once).await },
            |ws, result, cx| {
                match result {
                    Ok(()) => {
                        ws.modal = None;
                        ws.load_overview(cx);
                    }
                    // The setting may allow fewer than the protocol's most: the core says so.
                    Err(error) => {
                        if let Some(Modal::TeamAtOnce(form)) = &mut ws.modal {
                            form.error = Some(format!("{error:#}"));
                        }
                    }
                }
                cx.notify();
            },
        );
    }
}
