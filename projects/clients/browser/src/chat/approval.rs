//! What a chat waits on: a tool's request for a yes, and a question the agent asks.

use super::shown::{ReplyRow, question_labels};
use crate::{
    api,
    state::{Chat, state},
    ui::{Icon, IconName, run},
};
use dusk_protocol::tools::{ApprovalRequest, Decision, QuestionQueue};
use leptos::prelude::*;

/// One request at a time, with its place among the open ones: answering it shows the
/// next, and the arrows step through them, so none waits unseen behind another. A
/// subagent's or a team member's request is answered here too, named.
#[component]
pub fn Approvals(chat: Chat, id: Option<String>) -> impl IntoView {
    let shown = RwSignal::new(None::<u64>);
    let step = move |forward: bool| {
        let next = chat.with_untracked(|c| c.step_approval(shown.get_untracked(), forward));
        if next.is_some() {
            shown.set(next);
        }
    };
    let current = move || {
        chat.with(|c| {
            c.shown_approval(shown.get())
                .map(|(request, place, total)| {
                    (request.clone(), c.approval_asker(request.id), place, total)
                })
        })
    };
    view! {
        <div class="approvals">
            <For each=move || current().into_iter() key=|(a, _, place, total)| (a.id, *place, *total) let:shown_one>
                {
                    let (request, who, place, total) = shown_one;
                    view! {
                        <div class="approval">
                            {(total > 1).then(|| view! {
                                <div class="approval-place">
                                    <span class="muted">{format!("Approval {place} of {total}")}</span>
                                    <button class="icon" title="The request before" aria-label="The request before" on:click=move |_| step(false)>
                                        <Icon name=IconName::ChevronLeft />
                                    </button>
                                    <button class="icon" title="The next request" aria-label="The next request" on:click=move |_| step(true)>
                                        <Icon name=IconName::ChevronRight />
                                    </button>
                                </div>
                            })}
                            <ApprovalCard request who thread=id.clone().unwrap_or_default() />
                        </div>
                    }
                }
            </For>
        </div>
    }
}

#[component]
fn ApprovalCard(request: ApprovalRequest, who: String, thread: String) -> impl IntoView {
    let app = state();
    let id = request.id;
    let thread = StoredValue::new(thread);
    let lines: Vec<String> = request.steps.iter().map(|step| step.line()).collect();
    let session = request.session.is_some();
    let answer = move |decision: Decision| {
        run(api::approve(thread.get_value(), id, decision), move |_| {
            // The core closes the request itself; every client sees that event.
            let _ = app;
        });
    };
    view! {
        <div class="approval-body">
            <div class="approval-text">{request.title(&who)}</div>
            // A batch lists every step, numbered: the yes covers all of them.
            {(!lines.is_empty()).then(|| view! {
                <div class="approval-steps">
                    {lines.into_iter().map(|line| view! { <div>{line}</div> }).collect_view()}
                </div>
            })}
            <div class="approval-actions">
                <button class="primary" on:click=move |_| answer(Decision::Once)>"Allow once"</button>
                // Only what the core can keep for the session is offered, as the other clients do.
                {session.then(|| view! {
                    <button class="ghost" on:click=move |_| answer(Decision::Session)>"Allow for this chat"</button>
                })}
                <button class="ghost danger" on:click=move |_| answer(Decision::Deny)>"Deny"</button>
            </div>
        </div>
    }
}

/// Answers the question on the page through `act`: the next question shows, and the
/// answers go to the agent together after the last one.
fn record(chat: Chat, id: Option<String>, act: impl FnOnce(&mut QuestionQueue) -> Option<String>) {
    let mut all = None;
    chat.update(|c| all = c.answer_questions(act));
    if let Some(all) = all {
        state().send(id, all, false, Vec::new());
    }
}

/// The agent's question: pick from its options, add words of your own. A light one is
/// reply buttons; "Other..." opens the card for words of your own or a decline.
#[component]
pub fn AskUser(chat: Chat, id: Option<String>) -> impl IntoView {
    let chosen = RwSignal::new(Vec::<usize>::new());
    let own = RwSignal::new(String::new());
    let card = RwSignal::new(false);
    move || {
        let question = chat.with(|c| c.questions.shown().cloned())?;
        if !card.get()
            && let Some(labels) = question_labels(&question)
        {
            let skip = {
                let id = id.clone();
                move |_| record(chat, id.clone(), QuestionQueue::skip)
            };
            let id = id.clone();
            let pick = Callback::new(move |at: usize| {
                record(chat, id.clone(), |questions| questions.answer(&[at], ""));
            });
            let other = Callback::new(move |()| card.set(true));
            return Some(
                view! {
                    <div class="question light" role="group" aria-label="Question from the agent">
                        <div class="question-text">{question.question.clone()}</div>
                        <ReplyRow labels live=true pick other />
                        <div class="approval-actions">
                            <button class="ghost" on:click=skip>"Skip"</button>
                        </div>
                    </div>
                }
                .into_any(),
            );
        }
        let multiple = question.multiple;
        let (id, id_decline) = (id.clone(), id.clone());
        let skip = {
            let id = id.clone();
            move |_| {
                record(chat, id.clone(), QuestionQueue::skip);
                chosen.set(Vec::new());
                own.set(String::new());
                card.set(false);
            }
        };
        let submit = move |_| {
            let (picked, words) = (chosen.get_untracked(), own.get_untracked());
            record(chat, id.clone(), |questions| {
                questions.answer(&picked, &words)
            });
            chosen.set(Vec::new());
            own.set(String::new());
            card.set(false);
        };
        let decline = move |_| {
            let words = own.get_untracked();
            record(chat, id_decline.clone(), |questions| {
                questions.decline(&words)
            });
            card.set(false);
        };
        Some(
            view! {
                <div class="question">
                    <div class="question-text">{question.question.clone()}</div>
                    <div class="options">
                        {question.options.iter().enumerate().map(|(at, option)| {
                            let option = option.clone();
                            view! {
                                <label class="option">
                                    <input
                                        type=if multiple { "checkbox" } else { "radio" }
                                        name="answer"
                                        prop:checked=move || chosen.with(|c| c.contains(&at))
                                        on:change=move |_| chosen.update(|c| {
                                            if multiple {
                                                match c.iter().position(|x| *x == at) {
                                                    Some(p) => { c.remove(p); }
                                                    None => c.push(at),
                                                }
                                            } else {
                                                *c = vec![at];
                                            }
                                        })
                                    />
                                    {option}
                                </label>
                            }
                        }).collect_view()}
                    </div>
                    <input
                        class="own"
                        placeholder="Or say it your way"
                        prop:value=move || own.get()
                        on:input=move |ev| own.set(event_target_value(&ev))
                    />
                    <div class="approval-actions">
                        <button class="primary" on:click=submit>"Answer"</button>
                        <button class="ghost" on:click=skip>"Skip"</button>
                        <button class="ghost" on:click=decline>"Decline"</button>
                    </div>
                </div>
            }
            .into_any(),
        )
    }
}
