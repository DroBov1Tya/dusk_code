//! The shown chat's pinned plan: the Markdown the user and the model both keep. It reads
//! as a document; Edit turns it into a text box, and Save writes it back through the core.
//! The chat's older plans are listed under it, each deleted after a yes, or all at once.

use std::collections::HashSet;

use dusk_protocol::thread::PlanFile;
use gpui_kit::{
    component::{
        ActiveTheme, IconName, Sizable,
        button::{Button, ButtonVariants},
        input::{Textarea, TextareaState},
    },
    *,
};

use crate::{
    modal::Modal,
    pages::chat::links,
    ui::{Type, widgets},
    workspace::Workspace,
};

/// The name of the tool the model keeps the plan with.
pub const TOOL: &str = "plan_edit";

pub struct Plan {
    /// The plan of the chat in `chat`; none before one was written.
    pub text: Option<String>,
    pub chat: Option<String>,
    pub editing: bool,
    pub input: Entity<TextareaState>,
    /// Text for the box, put there on the next frame, which has the window.
    pub fill: Option<String>,
    /// Calls of the plan tool still running, so their end reloads the plan.
    pub calls: HashSet<String>,
    /// Every plan of the chat in `chat`, newest written first; the first is `text`'s.
    pub plans: Vec<PlanFile>,
}

/// What a yes in the confirmation deletes.
pub enum PlanDelete {
    One {
        thread_id: String,
        plan: PlanFile,
    },
    /// Every plan the core counts as old: replaced by a newer one, or finished.
    Old {
        thread_id: String,
        count: usize,
        /// The current plan is among them, being finished.
        current: bool,
    },
}

impl Plan {
    pub fn new(window: &mut Window, cx: &mut App) -> Self {
        Self {
            text: None,
            chat: None,
            editing: false,
            input: cx.new(|cx| {
                TextareaState::new(window, cx)
                    .auto_grow(6, 20)
                    .placeholder("The plan, as Markdown")
            }),
            fill: None,
            calls: HashSet::new(),
            plans: Vec::new(),
        }
    }
}

pub fn render(ws: &mut Workspace, window: &mut Window, cx: &mut Context<Workspace>) -> Div {
    if let Some(text) = ws.plan.fill.take() {
        ws.plan
            .input
            .update(cx, |input, cx| input.set_value(text, window, cx));
    }
    if ws.plan.editing {
        return div()
            .flex()
            .flex_col()
            .gap_2()
            .px_4()
            .child(
                div()
                    .p_2()
                    .rounded_md()
                    .border_1()
                    .border_color(cx.theme().border)
                    .child(Textarea::new(&ws.plan.input).appearance(false)),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap_2()
                    .child(
                        Button::new("plan-save")
                            .small()
                            .primary()
                            .label("Save")
                            .on_click(cx.listener(|ws, _, _, cx| ws.save_plan(cx))),
                    )
                    .child(
                        Button::new("plan-cancel")
                            .small()
                            .ghost()
                            .label("Cancel")
                            .on_click(cx.listener(|ws, _, _, cx| {
                                ws.plan.editing = false;
                                cx.notify();
                            })),
                    ),
            );
    }
    let text = ws.plan.text.clone().unwrap_or_default();
    let current = ws.plan.plans.first().filter(|plan| plan.current).cloned();
    let delete = current.map(|plan| {
        Button::new("plan-delete")
            .small()
            .ghost()
            .label("Delete")
            .tooltip("Delete this plan; the model stops reading it")
            .on_click(cx.listener(move |ws, _, _, cx| ws.ask_delete_plan(plan.clone(), cx)))
    });
    div()
        .flex()
        .flex_col()
        .gap_2()
        .px_4()
        .child(links::markdown("plan-view", text, links::Links::Paths))
        .child(
            div()
                .flex()
                .flex_row()
                .gap_1()
                .child(
                    Button::new("plan-edit")
                        .small()
                        .ghost()
                        .label("Edit")
                        .tooltip("Change the plan; the model reads it with every message")
                        .on_click(cx.listener(|ws, _, _, cx| ws.edit_plan(cx))),
                )
                .children(delete),
        )
        .children(older(ws, cx))
}

/// The chat's other plans, each with its delete, and the way to delete every old one.
fn older(ws: &Workspace, cx: &mut Context<Workspace>) -> Option<Div> {
    let others: Vec<&PlanFile> = ws.plan.plans.iter().filter(|plan| !plan.current).collect();
    let old = ws.plan.plans.iter().filter(|plan| plan.old()).count();
    if others.is_empty() && old == 0 {
        return None;
    }
    let muted = cx.theme().muted_foreground;
    let mut list = div().flex().flex_col().gap_0p5();
    for plan in others {
        let what = if plan.finished {
            "finished"
        } else {
            "replaced"
        };
        let chosen = plan.clone();
        list = list.child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .py_0p5()
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_row()
                        .gap_2()
                        .child(div().small().truncate().child(plan.name.clone()))
                        .child(
                            div()
                                .meta()
                                .flex_none()
                                .text_color(muted)
                                .child(format!("{what} \u{00B7} {}", widgets::short(plan.bytes))),
                        ),
                )
                .child(
                    Button::new(SharedString::from(format!("plan-drop-{}", plan.source_id)))
                        .ghost()
                        .xsmall()
                        .icon(IconName::Delete)
                        .tooltip("Delete this plan")
                        .on_click(
                            cx.listener(move |ws, _, _, cx| ws.ask_delete_plan(chosen.clone(), cx)),
                        ),
                ),
        );
    }
    let clear = (old > 0).then(|| {
        Button::new("plan-drop-old")
            .small()
            .ghost()
            .label(if old == 1 {
                "Delete the old plan".to_owned()
            } else {
                format!("Delete {old} old plans")
            })
            .tooltip("Delete the plans a newer one replaced and the finished ones")
            .on_click(cx.listener(|ws, _, _, cx| ws.ask_delete_old_plans(cx)))
    });
    Some(
        div()
            .flex()
            .flex_col()
            .gap_1()
            .pt_2()
            .border_t_1()
            .border_color(cx.theme().border.opacity(0.5))
            .child(div().meta().text_color(muted).child("Earlier plans"))
            .child(list)
            .children(clear.map(|clear| div().flex().flex_row().child(clear))),
    )
}

/// The yes a plan's deletion asks for: plainer for the current plan, which the model
/// reads with every message.
pub fn confirm(delete: &PlanDelete, cx: &mut Context<Workspace>) -> Div {
    let muted = cx.theme().muted_foreground;
    let (question, detail, yes) = match delete {
        PlanDelete::One { plan, .. } if plan.current => (
            format!("Delete the current plan \"{}\"?", plan.name),
            "The model reads it with every message; after this it no longer does, and the \
             Plan screen shows the plan written before it, if any. This cannot be undone."
                .to_owned(),
            "Delete the current plan".to_owned(),
        ),
        PlanDelete::One { plan, .. } => (
            format!("Delete \"{}\"?", plan.name),
            "The model no longer gets it. This cannot be undone.".to_owned(),
            "Delete".to_owned(),
        ),
        PlanDelete::Old { count, current, .. } => (
            match count {
                1 => "Delete the old plan?".to_owned(),
                count => format!("Delete {count} old plans?"),
            },
            if *current {
                "Plans a newer one replaced and finished ones go, the current plan too: \
                 every box of it is ticked. This cannot be undone."
            } else {
                "Plans a newer one replaced and finished ones go; the current plan stays. \
                 This cannot be undone."
            }
            .to_owned(),
            "Delete old plans".to_owned(),
        ),
    };
    div()
        .flex()
        .flex_col()
        .gap_3()
        .p_5()
        .child(question)
        .child(div().small().text_color(muted).child(detail))
        .child(
            div()
                .flex()
                .flex_row()
                .justify_end()
                .gap_2()
                .child(
                    Button::new("plan-delete-cancel")
                        .ghost()
                        .label("Keep")
                        .on_click(cx.listener(|ws, _, _, cx| ws.close_modal(cx))),
                )
                .child(
                    Button::new("plan-delete-confirm")
                        .danger()
                        .label(yes)
                        .on_click(cx.listener(|ws, _, _, cx| ws.delete_plans(cx))),
                ),
        )
}

impl Workspace {
    /// Reads the shown chat's plan again.
    pub fn load_plan(&mut self, cx: &mut Context<Self>) {
        let Some(thread_id) = self.shown.clone() else {
            return;
        };
        let asked = thread_id.clone();
        self.core.call(
            cx,
            move |client| async move {
                let work = client.work();
                Ok((work.plan(&thread_id).await?, work.plans(&thread_id).await?))
            },
            move |ws, result, cx| match result {
                // The answer of a chat left since is not this chat's.
                Ok((text, plans)) if ws.shown.as_deref() == Some(asked.as_str()) => {
                    // Words typed in the box are not overwritten by the model's edit.
                    if !ws.plan.editing {
                        ws.plan.text = text;
                    }
                    ws.plan.plans = plans;
                    ws.plan.chat = Some(asked);
                    cx.notify();
                }
                Ok(_) => {}
                Err(error) => ws.fail(error, cx),
            },
        );
    }

    /// Opens the box on the plan as it stands, or on an empty one.
    pub fn edit_plan(&mut self, cx: &mut Context<Self>) {
        self.plan.fill = Some(self.plan.text.clone().unwrap_or_default());
        self.plan.editing = true;
        self.folded.remove(&crate::workspace::Right::Plan);
        self.right_hidden = false;
        cx.notify();
    }

    fn save_plan(&mut self, cx: &mut Context<Self>) {
        let Some(thread_id) = self.shown.clone() else {
            return;
        };
        let text = self.plan.input.read(cx).value().to_string();
        self.core.call(
            cx,
            move |client| async move {
                client.work().save_plan(&thread_id, &text).await?;
                Ok(text)
            },
            |ws, result, cx| match result {
                Ok(text) => {
                    ws.plan.text = (!text.trim().is_empty()).then_some(text);
                    ws.plan.editing = false;
                    cx.notify();
                }
                Err(error) => ws.fail(error, cx),
            },
        );
    }

    pub fn ask_delete_plan(&mut self, plan: PlanFile, cx: &mut Context<Self>) {
        if let Some(thread_id) = self.shown.clone() {
            self.modal = Some(Modal::DeletePlan(PlanDelete::One { thread_id, plan }));
            cx.notify();
        }
    }

    pub fn ask_delete_old_plans(&mut self, cx: &mut Context<Self>) {
        let Some(thread_id) = self.shown.clone() else {
            return;
        };
        let old = self.plan.plans.iter().filter(|plan| plan.old());
        let (count, current) = old.fold((0, false), |(count, current), plan| {
            (count + 1, current || plan.current)
        });
        self.modal = Some(Modal::DeletePlan(PlanDelete::Old {
            thread_id,
            count,
            current,
        }));
        cx.notify();
    }

    /// Deletes what the confirmation names, then reads the plans and sources again.
    fn delete_plans(&mut self, cx: &mut Context<Self>) {
        let Some(Modal::DeletePlan(delete)) = self.modal.take() else {
            return;
        };
        cx.notify();
        self.core.call(
            cx,
            move |client| async move {
                match delete {
                    PlanDelete::One { thread_id, plan } => {
                        client
                            .work()
                            .remove_source(&thread_id, &plan.source_id)
                            .await
                    }
                    PlanDelete::Old { thread_id, .. } => {
                        client.work().remove_old_plans(&thread_id).await.map(drop)
                    }
                }
            },
            |ws, result, cx| match result {
                Ok(()) => {
                    ws.load_plan(cx);
                    ws.load_sources(cx);
                }
                Err(error) => ws.fail(error, cx),
            },
        );
    }

    /// Notes the model's plan calls, and reads the plan once one has finished.
    pub fn watch_plan(
        &mut self,
        thread_id: &str,
        event: &dusk_protocol::thread::ThreadEvent,
        cx: &mut Context<Self>,
    ) {
        use dusk_protocol::thread::ThreadEvent;
        match event {
            ThreadEvent::ToolStarted(call) if call.name == TOOL => {
                self.plan.calls.insert(call.id.clone());
            }
            ThreadEvent::ToolFinished { call_id, .. }
                if self.plan.calls.remove(call_id) && self.shown.as_deref() == Some(thread_id) =>
            {
                self.load_plan(cx);
            }
            _ => {}
        }
    }
}

/// Shown while the chat has a plan, or the user is writing one.
pub fn has_content(ws: &Workspace) -> bool {
    ws.plan.editing || ws.plan.text.is_some()
}
