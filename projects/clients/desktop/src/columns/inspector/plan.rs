//! The shown chat's pinned plan: the Markdown the user and the model both keep. It reads
//! as a document; Edit turns it into a text box, and Save writes it back through the core.

use std::collections::HashSet;

use gpui_kit::{
    component::{
        ActiveTheme, Sizable,
        button::{Button, ButtonVariants},
        input::{Textarea, TextareaState},
    },
    *,
};

use crate::{pages::chat::links, workspace::Workspace};

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
    div()
        .flex()
        .flex_col()
        .gap_2()
        .px_4()
        .child(links::markdown("plan-view", text))
        .child(
            div().flex().flex_row().child(
                Button::new("plan-edit")
                    .small()
                    .ghost()
                    .label("Edit")
                    .tooltip("Change the plan; the model reads it with every message")
                    .on_click(cx.listener(|ws, _, _, cx| ws.edit_plan(cx))),
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
            move |client| async move { client.work().plan(&thread_id).await },
            move |ws, result, cx| match result {
                // The answer of a chat left since is not this chat's.
                Ok(text) if ws.shown.as_deref() == Some(asked.as_str()) => {
                    // Words typed in the box are not overwritten by the model's edit.
                    if !ws.plan.editing {
                        ws.plan.text = text;
                    }
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
