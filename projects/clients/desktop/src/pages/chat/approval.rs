//! What waits for the user: a tool's request for a yes, and a question of the agent.

use crate::ui::elevation::{Elevation, Level};
use std::rc::Rc;

use dusk_protocol::tools::{Decision, Question, Request};
use gpui_kit::{
    component::{
        ActiveTheme, IconName, Sizable,
        button::{Button, ButtonVariants},
    },
    prelude::FluentBuilder,
    *,
};

use crate::{
    pages::chat::{messages, transcript::Transcript},
    ui::{Type, widgets},
    workspace::Workspace,
};

/// Lines of a command shown before its box scrolls.
const SHOWN_LINES: f32 = 3.;
const LINE: f32 = 18.;
/// Room the buttons row and the card's padding take.
const BUTTONS: f32 = 72.;

/// The dock never takes more than half the window; each card fits in it with its buttons,
/// so a long command scrolls inside its box and the yes and no stay in view.
pub fn render(ws: &Workspace, window: &Window, cx: &mut Context<Workspace>) -> Stateful<Div> {
    let dock = (window.viewport_size().height * 0.5).max(px(160.));
    let budget = (dock - px(BUTTONS)).max(px(LINE * SHOWN_LINES + 16.));
    let mut cards = div()
        .id("approval-dock")
        .max_h(dock)
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .gap_2();
    let Some(chat) = ws.shown_chat() else {
        return cards;
    };
    let theme = cx.theme();
    let (border, warning) = (theme.border, theme.warning);
    let thread_id = ws.shown.clone().unwrap_or_default();
    // One request at a time, with its place among the open ones: answering it shows the
    // next, and the arrows step through them, so none waits unseen behind another.
    if let Some((request, place, total)) = chat.shown_approval(ws.approval_shown) {
        let id = request.id;
        // A subagent's or a team member's request is answered here too, named.
        let who = chat.approval_asker(id);
        let (once, session, deny) = (thread_id.clone(), thread_id.clone(), thread_id.clone());
        let mut buttons = div().flex().flex_row().gap_2().child(
            Button::new(SharedString::from(format!("once-{id}")))
                .primary()
                .label("Allow once")
                .on_click(
                    cx.listener(move |ws, _, _, cx| ws.approve(&once, id, Decision::Once, cx)),
                ),
        );
        if let Some(scope) = &request.session {
            buttons = buttons.child(
                Button::new(SharedString::from(format!("session-{id}")))
                    .label(format!("Allow {scope} for the session"))
                    .on_click(cx.listener(move |ws, _, _, cx| {
                        ws.approve(&session, id, Decision::Session, cx)
                    })),
            );
        }
        buttons = buttons.child(
            Button::new(SharedString::from(format!("deny-{id}")))
                // One filled button on the card: the yes. The no is its outline, in the
                // danger color, so it is found and not pressed by habit.
                .danger()
                .outline()
                .label("Deny")
                .on_click(
                    cx.listener(move |ws, _, _, cx| ws.approve(&deny, id, Decision::Deny, cx)),
                ),
        );
        // A command is shown as code: it came from the model and is about to run. Its box
        // shows three lines and scrolls; "Show all" opens it up to the card's room.
        // A batch shows every step, one row each: the yes covers all of them.
        let what = match &request.request {
            // A long batch scrolls inside the card, so the buttons stay in view.
            _ if !request.steps.is_empty() => {
                let steps = request.steps.iter().fold(
                    div()
                        .id(SharedString::from(format!("steps-{id}")))
                        .max_h(budget - px(24.))
                        .overflow_y_scroll()
                        .flex()
                        .flex_col()
                        .gap_1(),
                    |list, step| match &step.request {
                        Request::Command { command } if !step.allowed => list.child(
                            div()
                                .pl_3()
                                .flex()
                                .flex_col()
                                .gap_1()
                                .child(format!("{}.", step.number))
                                .child(code(ws, (id, step.number), command, budget, cx)),
                        ),
                        _ => list.child(div().pl_3().child(step.line())),
                    },
                );
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(request.title(&who))
                    .child(steps)
            }
            Request::Command { command } => div()
                .flex()
                .flex_col()
                .gap_2()
                .child(format!("{who} wants to run"))
                .child(code(ws, (id, 0), command, budget - px(24.), cx)),
            other => div().child(other.describe(&who)),
        };
        cards = cards.child(
            card(cx)
                .border_color(warning.opacity(0.6))
                .when(total > 1, |card| card.child(stepper(place, total, cx)))
                .child(what)
                .child(buttons.flex_none()),
        );
    }
    if let Some(question) = chat.questions.shown().filter(|question| buttons(question)) {
        let (asked, total) = chat.questions.place();
        cards = cards.child(light_question(question, asked, total, cx));
    } else if let Some(question) = chat.questions.shown() {
        let muted = cx.theme().muted_foreground;
        let (primary, hover) = (cx.theme().primary, muted.opacity(0.5));
        let mut options = div().flex().flex_col().gap_2();
        for (index, option) in question.options.iter().enumerate() {
            let picked = ws.picked.contains(&index);
            // A tick for what is chosen in a list of several, the number otherwise.
            let badge = if question.multiple && picked {
                widgets::icon(IconName::CircleCheck, primary).into_any_element()
            } else {
                div()
                    .flex_none()
                    .size(px(24.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .border_1()
                    .border_color(if picked { primary } else { border })
                    .meta()
                    .text_color(if picked { primary } else { muted })
                    .child((index + 1).to_string())
                    .into_any_element()
            };
            options = options.child(
                div()
                    .id(("option", index))
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_3()
                    .px_3()
                    .py_2()
                    .rounded_lg()
                    .border_1()
                    .border_color(if picked { primary } else { border })
                    .when(picked, |card| card.bg(primary.opacity(0.08)))
                    .cursor_pointer()
                    .hover(move |card| card.border_color(if picked { primary } else { hover }))
                    .child(badge)
                    .child(div().flex_1().min_w_0().child(option.clone()))
                    .on_click(cx.listener(move |ws, _, _, cx| ws.pick(index, cx))),
            );
        }
        let mut card = card(cx)
            .child(
                div()
                    .font_weight(FontWeight::MEDIUM)
                    .child(question.question.clone()),
            )
            .child(div().meta().text_color(muted).child(if question.multiple {
                "Pick any. Or write your own answer below."
            } else {
                "Pick one. Or write your own answer below."
            }))
            // More wait behind this one; the answers go to the agent together.
            .when(chat.questions.place().1 > 1, |card| {
                let (number, total) = chat.questions.place();
                card.child(
                    div()
                        .meta()
                        .text_color(muted)
                        .child(format!("Question {number} of {total}")),
                )
            })
            .child(options);
        let mut actions = div().flex().flex_row().items_center().gap_2();
        if question.multiple {
            actions = actions.child(
                Button::new("answer-chosen")
                    .primary()
                    .label("Answer")
                    .on_click(cx.listener(|ws, _, _, cx| ws.answer_picked(cx))),
            );
        }
        card = card.child(actions.child(skip_button(cx)));
        cards = cards.child(card);
    }
    cards
}

/// `Approval 2 of 3` with the arrows to the one before and after.
fn stepper(place: usize, total: usize, cx: &mut Context<Workspace>) -> Div {
    let muted = cx.theme().muted_foreground;
    let step = |name: &'static str, icon: IconName, tip: &'static str, forward: bool| {
        Button::new(name)
            .ghost()
            .xsmall()
            .icon(icon)
            .tooltip(tip)
            .on_click(cx.listener(move |ws, _, _, cx| ws.step_approval(forward, cx)))
    };
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap_1()
        .child(
            div()
                .flex_1()
                .meta()
                .text_color(muted)
                .child(format!("Approval {place} of {total}")),
        )
        .child(step(
            "approval-before",
            IconName::ChevronLeft,
            "The request before",
            false,
        ))
        .child(step(
            "approval-after",
            IconName::ChevronRight,
            "The next request",
            true,
        ))
}

/// A command in a box of three lines that scrolls inside; long lines wrap. `key` is the
/// request and step, so each box opens on its own; open, it grows up to `room`.
fn code(
    ws: &Workspace,
    key: (u64, usize),
    command: &str,
    room: Pixels,
    cx: &mut Context<Workspace>,
) -> Div {
    let mono = crate::ui::theme::prefs(cx)
        .mono_font
        .clone()
        .unwrap_or_else(|| "Menlo".to_owned());
    let open = ws.approval_open.contains(&key);
    let shut = px(LINE * SHOWN_LINES + 16.);
    let long = long(command);
    let height = if open {
        (room - px(28.)).max(shut)
    } else {
        shut
    };
    let (id, step) = key;
    div()
        .flex()
        .flex_col()
        .gap_1()
        .child(
            div()
                .id(SharedString::from(format!("command-{id}-{step}")))
                .max_h(height)
                .overflow_y_scroll()
                .px_3()
                .py_2()
                .rounded_lg()
                .bg(cx.theme().background)
                .small()
                .line_height(px(LINE))
                .font_family(mono)
                .whitespace_normal()
                .child(command.to_owned()),
        )
        .when(long, |column| {
            column.child(
                div()
                    .id(SharedString::from(format!("command-more-{id}-{step}")))
                    .meta()
                    .text_color(cx.theme().muted_foreground)
                    .cursor_pointer()
                    .child(if open { "Show less" } else { "Show all" })
                    .on_click(cx.listener(move |ws, _, _, cx| {
                        if !ws.approval_open.remove(&key) {
                            ws.approval_open.insert(key);
                        }
                        cx.notify();
                    })),
            )
        })
}

/// More than the box shows: over three lines, or a line long enough to wrap past them.
fn long(command: &str) -> bool {
    command.lines().count() > SHOWN_LINES as usize || command.chars().count() > 240
}

/// A short question is answered with one click on a pill, not in a card.
fn buttons(question: &Question) -> bool {
    question.light()
}

/// A question of 2 to 4 short options as reply buttons, with "Other..." to write one's own.
fn light_question(
    question: &Question,
    asked: usize,
    total: usize,
    cx: &mut Context<Workspace>,
) -> Div {
    let labels: Vec<String> = question
        .options
        .iter()
        .map(|option| option.trim().to_owned())
        .collect();
    let count = labels.len();
    let pick: messages::Pick = Rc::new(move |ws, index, window, cx| {
        if index < count {
            ws.pick(index, cx);
        } else {
            ws.composer
                .update(cx, |composer, cx| composer.focus(window, cx));
        }
    });
    let muted = cx.theme().muted_foreground;
    div()
        .flex()
        .flex_col()
        .gap_2()
        .child(
            div()
                .small()
                .font_weight(FontWeight::MEDIUM)
                .child(question.question.clone()),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .items_start()
                .gap_2()
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .child(messages::pills("question", &labels, true, true, pick, cx)),
                )
                .child(skip_button(cx)),
        )
        // More wait behind this one; the answers go to the agent together.
        .when(total > 1, |column| {
            column.child(
                div()
                    .meta()
                    .text_color(muted)
                    .child(format!("Question {asked} of {total}")),
            )
        })
}

/// Leaves the question to the agent: a quiet button, since the choice is the main thing.
fn skip_button(cx: &mut Context<Workspace>) -> Button {
    Button::new("skip-question")
        .ghost()
        .label("Skip")
        .on_click(cx.listener(|ws, _, _, cx| ws.skip_question(cx)))
}

fn card(cx: &App) -> Div {
    div()
        .flex()
        .flex_col()
        .gap_3()
        .p_3()
        .rounded_xl()
        .elevation(Level::Raised, cx)
}

impl Workspace {
    fn step_approval(&mut self, forward: bool, cx: &mut Context<Self>) {
        let shown = self.approval_shown;
        if let Some(next) = self
            .shown_chat()
            .and_then(|chat| chat.step_approval(shown, forward))
        {
            self.approval_shown = Some(next);
            cx.notify();
        }
    }

    fn approve(&mut self, thread_id: &str, id: u64, decision: Decision, cx: &mut Context<Self>) {
        self.core.call(
            cx,
            {
                let thread_id = thread_id.to_owned();
                move |client| async move { client.threads().approve(&thread_id, id, decision).await }
            },
            move |ws, result, cx| {
                if let Err(error) = result {
                    ws.fail(error, cx);
                }
                // The core closes it with an event as well; this hides it at once.
                for chat in ws.chats.values_mut() {
                    chat.close_approval(id);
                }
                cx.notify();
            },
        );
    }

    /// One choice answers at once; with several allowed, it is marked until Answer.
    fn pick(&mut self, index: usize, cx: &mut Context<Self>) {
        let multiple = self
            .shown_chat()
            .and_then(|chat| chat.questions.shown())
            .is_some_and(|q| q.multiple);
        if !multiple {
            self.picked = vec![index];
            self.answer_picked(cx);
            return;
        }
        if let Some(at) = self.picked.iter().position(|&picked| picked == index) {
            self.picked.remove(at);
        } else {
            self.picked.push(index);
        }
        cx.notify();
    }

    fn answer_picked(&mut self, cx: &mut Context<Self>) {
        let chosen = std::mem::take(&mut self.picked);
        if chosen.is_empty() {
            return;
        }
        self.answer_with(|chat| chat.answer(&chosen, ""), cx);
    }

    /// Skips only the question shown; the next one shows, or the answers go out.
    fn skip_question(&mut self, cx: &mut Context<Self>) {
        self.picked.clear();
        self.answer_with(Transcript::skip, cx);
    }

    fn answer_with(
        &mut self,
        record: impl FnOnce(&mut Transcript) -> Option<String>,
        cx: &mut Context<Self>,
    ) {
        let Some(thread_id) = self.shown.clone() else {
            return;
        };
        let Some(chat) = self.chats.get_mut(&thread_id) else {
            return;
        };
        let Some(text) = record(chat) else {
            // The next question shows; nothing is sent until the last one is answered.
            cx.notify();
            return;
        };
        self.core.call(
            cx,
            move |client| async move { client.threads().send(&thread_id, text, false).await },
            |ws, result, cx| {
                if let Err(error) = result {
                    ws.fail(error, cx);
                }
            },
        );
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use dusk_protocol::tools::Question;

    use super::buttons;
    use crate::pages::chat::transcript::Transcript;

    fn question(options: &[&str], multiple: bool) -> Question {
        Question {
            question: "Which?".into(),
            options: options.iter().map(ToString::to_string).collect(),
            multiple,
        }
    }

    #[test]
    fn a_light_question_becomes_buttons_and_any_other_stays_a_card() {
        assert!(buttons(&question(&["Yes", "No"], false)));
        assert!(!buttons(&question(&["Yes", "No"], true)));
        assert!(!buttons(&question(&["a", "b", "c", "d", "e"], false)));
        assert!(!buttons(&question(&["Yes"], false)));
        assert!(!buttons(&question(&["Yes", &"x".repeat(25)], false)));
    }

    #[test]
    fn skipping_moves_on_and_the_last_skip_sends_every_answer() {
        let (first, second) = (question(&["a", "b"], false), question(&["c", "d"], true));
        let mut chat = Transcript::default();
        chat.questions.ask(first.clone());
        chat.questions.ask(second.clone());
        assert_eq!(chat.skip(), None);
        assert_eq!(chat.questions.shown(), Some(&second));
        assert_eq!(chat.questions.place(), (2, 2));
        let sent = chat.answer(&[1], "").unwrap();
        assert_eq!(
            sent,
            [first.skipped(), second.answer(&[1], "")].join("\n\n")
        );
        assert!(chat.questions.is_empty());

        chat.questions.ask(first.clone());
        chat.questions.ask(second.clone());
        assert_eq!(chat.answer(&[0], ""), None);
        let sent = chat.skip().unwrap();
        assert_eq!(
            sent,
            [first.answer(&[0], ""), second.skipped()].join("\n\n")
        );
        assert_eq!(chat.skip(), None, "nothing is left to skip");
    }
}
