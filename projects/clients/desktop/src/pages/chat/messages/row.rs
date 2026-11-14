//! One row of the list: the user's words, an answer, a command, a picture, a notice;
//! and going back to a sent message.

use gpui_kit::{
    component::{
        ActiveTheme, Disableable, IconName, Sizable,
        button::{Button, ButtonVariants},
    },
    prelude::FluentBuilder,
    *,
};

use super::{
    Row,
    activity::{agent_report, turn_activity},
    command::command_card,
    cost::TurnCost,
    picture::picture,
};
use crate::{
    pages::chat::{links, transcript::Item, working},
    ui::Type,
    workspace::Workspace,
};

impl Workspace {
    pub fn transcript_row(&mut self, ix: usize, cx: &mut Context<Self>) -> AnyElement {
        let shown = self.shown.clone().unwrap_or_default();
        let (Some(row), Some(chat)) = (self.rows.get(ix).copied(), self.chats.get(&shown)) else {
            return div().into_any_element();
        };
        let theme = cx.theme();
        let (muted, accent, danger) = (theme.muted_foreground, theme.primary, theme.danger);
        // Space between rows lives in the rows, as the list has no gap of its own.
        let content = match row {
            Row::Working => div()
                .flex()
                .flex_row()
                .items_center()
                .gap_3()
                .meta()
                .text_color(muted)
                .child(working::render(
                    SharedString::from(format!("working-{shown}")),
                    accent,
                    theme.foreground,
                ))
                .child(match chat.span(chat.activity_boundaries.len()) {
                    Some((began, None)) => format!(
                        "Working \u{00B7} {}",
                        crate::pages::chat::timing::duration(
                            dusk_protocol::system::now_ms() - began
                        )
                    ),
                    _ => "Working".to_owned(),
                })
                .into_any_element(),
            Row::Activity {
                start,
                end,
                span,
                spent,
            } => {
                let dear = spent
                    .zip(chat.mean_turn_cost())
                    .is_some_and(|((_, micro), mean)| micro > 2 * mean.max(1));
                turn_activity(self, &shown, start, &chat.items[start..end], span, cx)
                    .child_cost(spent, dear, cx)
                    .into_any_element()
            }
            Row::Item(at) => {
                let key = format!("{shown}-{at}");
                match &chat.items[at] {
                    Item::User { text, sequence } => {
                        let group = SharedString::from(format!("user-row-{key}"));
                        let mut tools = div()
                            .flex()
                            .flex_row()
                            .gap_1()
                            .invisible()
                            .group_hover(group.clone(), |tools| tools.visible());
                        let copied = text.clone();
                        tools = tools.child(
                            Button::new(SharedString::from(format!("copy-{key}")))
                                .ghost()
                                .small()
                                .icon(IconName::Copy)
                                .tooltip("Copy the message")
                                .on_click(cx.listener(move |_, _, _, cx| {
                                    cx.write_to_clipboard(ClipboardItem::new_string(copied.clone()))
                                })),
                        );
                        // The core refuses to go back inside a turn, so the button waits.
                        if let (Some(sequence), false) = (*sequence, chat.working) {
                            let (thread_id, text) = (shown.clone(), text.clone());
                            tools = tools.child(
                                Button::new(SharedString::from(format!("rollback-{key}")))
                                    .ghost()
                                    .small()
                                    .icon(IconName::Undo2)
                                    .tooltip("Go back to this message")
                                    .on_click(cx.listener(move |ws, _, _, cx| {
                                        ws.ask_rollback(&thread_id, sequence, at, &text, cx)
                                    })),
                            );
                        }
                        div()
                            .id(group.clone())
                            .group(group)
                            .self_end()
                            .max_w(px(600.))
                            .flex()
                            .flex_col()
                            .items_end()
                            .gap_1()
                            .child(
                                // The user's words carry a breath of the accent, so the
                                // two sides of the chat part at a glance without glare.
                                div()
                                    .px_4()
                                    .py_2()
                                    .rounded_xl()
                                    .bg(accent.opacity(0.09))
                                    .border_1()
                                    .border_color(accent.opacity(0.16))
                                    // Selectable like the answers; pastes and files show as labels.
                                    .child(links::markdown(
                                        SharedString::from(format!("user-{key}")),
                                        user_text(text),
                                    )),
                            )
                            .child(tools)
                            .into_any_element()
                    }
                    Item::Command(run) => command_card(&key, run, cx),
                    Item::Agent { from, text } => {
                        agent_report(self, &shown, at, from, text, cx).into_any_element()
                    }
                    Item::Assistant { text, .. } => {
                        let group = SharedString::from(format!("answer-row-{key}"));
                        let mut answer = div()
                            .id(group.clone())
                            .group(group.clone())
                            .flex()
                            .flex_col()
                            .gap_1();
                        if !text.is_empty() {
                            answer = answer.child(crate::pages::chat::media::render(
                                &format!("answer-{key}"),
                                text,
                                cx,
                            ));
                            let (copied, spoken) = (text.clone(), text.clone());
                            let listening = self.audio.listening.as_deref() == Some(key.as_str());
                            let (spoken_key, other_busy) =
                                (key.clone(), self.audio.listening.is_some() && !listening);
                            // The answer reads as plain text; its tools come up under the
                            // pointer, or while it is read aloud.
                            answer = answer.child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .gap_1()
                                    .when(!listening, |row| {
                                        row.opacity(0.)
                                            .group_hover(group.clone(), |row| row.opacity(1.))
                                    })
                                    .child(
                                        Button::new(SharedString::from(format!("copy-{key}")))
                                            .ghost()
                                            .small()
                                            .icon(IconName::Copy)
                                            .tooltip("Copy the answer")
                                            .on_click(cx.listener(move |_, _, _, cx| {
                                                cx.write_to_clipboard(ClipboardItem::new_string(
                                                    copied.clone(),
                                                ))
                                            })),
                                    )
                                    .child(
                                        Button::new(SharedString::from(format!("listen-{key}")))
                                            .ghost()
                                            .small()
                                            .icon(if listening {
                                                IconName::Square
                                            } else {
                                                IconName::Play
                                            })
                                            .label(if listening { "Stop" } else { "Listen" })
                                            .disabled(other_busy)
                                            .tooltip(if listening {
                                                "Stop reading"
                                            } else {
                                                "Read the answer aloud"
                                            })
                                            .on_click(cx.listener(move |ws, _, _, cx| {
                                                if listening {
                                                    ws.stop_listening(cx);
                                                } else {
                                                    ws.speak_answer(&spoken_key, &spoken, cx);
                                                }
                                            })),
                                    ),
                            );
                        }
                        answer.into_any_element()
                    }
                    Item::Image(image) => picture(&self.status.artifacts_dir, image, cx),
                    Item::Audio(clip) => crate::pages::chat::audio::card(self, &key, clip, cx),
                    Item::Notice(text) => div()
                        .meta()
                        .text_color(muted)
                        .child(text.clone())
                        .into_any_element(),
                    Item::Error(text) => div()
                        .px_3()
                        .py_2()
                        .rounded_md()
                        .meta()
                        .text_color(danger)
                        .bg(danger.opacity(0.1))
                        .child(text.clone())
                        .into_any_element(),
                    Item::Tool(_) => div().into_any_element(),
                }
            }
        };
        let activity = matches!(row, Row::Activity { .. });
        let gutter = if activity { 3. } else { 8. };
        let top = if ix == 0 { 24. } else { gutter };
        let bottom = if ix + 1 == self.rows.len() {
            24.
        } else {
            gutter
        };
        div()
            .pt(px(top))
            .pb(px(bottom))
            .child(lane(content))
            .into_any_element()
    }
}

/// A sent message as the user wrote it, each paste and kept file as a short label.
fn user_text(text: &str) -> String {
    use dusk_protocol::thread::{Part, split};
    split(text)
        .into_iter()
        .map(|part| match part {
            Part::Text(text) => text.to_owned(),
            Part::Pasted(pasted) => {
                format!("`[{}]`", crate::pages::chat::composer::paste_label(pasted))
            }
            Part::Source(name) => format!("`[{name}]`"),
        })
        .collect()
}

/// A message as one short line for a list: attachments by name, and an answer to the
/// agent's question as the answer alone.
pub fn label(text: &str) -> String {
    use dusk_protocol::thread::{Part, split};
    let text = match text
        .strip_prefix("QUESTION: ")
        .and_then(|rest| rest.split_once("\nANSWER: "))
    {
        Some((_, answer)) => format!("Answer: {answer}"),
        None => text.to_owned(),
    };
    let plain: String = split(&text)
        .into_iter()
        .map(|part| match part {
            Part::Text(text) => text.to_owned(),
            Part::Pasted(pasted) => {
                format!("[{}]", crate::pages::chat::composer::paste_label(pasted))
            }
            Part::Source(name) => format!("[{name}]"),
        })
        .collect();
    plain.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A row at the chat's reading width, centered.
fn lane(content: AnyElement) -> AnyElement {
    div()
        .w_full()
        .flex()
        .justify_center()
        .px_6()
        .child(
            div()
                .w_full()
                .max_w(px(780.))
                .flex()
                .flex_col()
                .child(content),
        )
        .into_any_element()
}

impl Workspace {
    fn ask_rollback(
        &mut self,
        thread_id: &str,
        sequence: u64,
        at: usize,
        text: &str,
        cx: &mut Context<Self>,
    ) {
        self.modal = Some(crate::modal::Modal::Rollback {
            thread_id: thread_id.to_owned(),
            sequence,
            at,
            text: text.to_owned(),
        });
        cx.notify();
    }

    /// Takes the chat back to before the message: the core forgets it and all after it,
    /// and the view is cut when the core says so. The message returns to the box.
    pub fn rollback(&mut self, cx: &mut Context<Self>) {
        let Some(crate::modal::Modal::Rollback {
            thread_id,
            sequence,
            at,
            text,
        }) = self.modal.take()
        else {
            return;
        };
        if let Some(chat) = self.chats.get_mut(&thread_id) {
            chat.rollback_at = Some(at);
        }
        let id = thread_id.clone();
        self.core.call(
            cx,
            move |client| async move {
                client
                    .threads()
                    .send(&thread_id, format!("/undo {sequence}"), false)
                    .await
            },
            move |ws, result, cx| match result {
                Ok(()) => ws.fill_composer = Some(text),
                Err(error) => {
                    if let Some(chat) = ws.chats.get_mut(&id) {
                        chat.rollback_at = None;
                    }
                    ws.fail(error, cx);
                }
            },
        );
        cx.notify();
    }
}
