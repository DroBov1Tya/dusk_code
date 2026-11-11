//! What a chat waits on: a tool's request for a yes, and a question the agent asks.

use crate::{
    api,
    state::{Chat, state},
    ui::run,
};
use dusk_protocol::tools::{ApprovalRequest, Decision};
use leptos::prelude::*;

#[component]
pub fn Approvals(chat: Chat) -> impl IntoView {
    view! {
        <div class="approvals">
            <For each=move || chat.with(|c| c.approvals.clone()) key=|a| a.id let:request>
                <ApprovalCard request />
            </For>
        </div>
    }
}

#[component]
fn ApprovalCard(request: ApprovalRequest) -> impl IntoView {
    let app = state();
    let id = request.id;
    let answer = move |decision: Decision| {
        run(api::approve(id, decision), move |_| {
            // The core closes the request itself; every client sees that event.
            let _ = app;
        });
    };
    view! {
        <div class="approval">
            <div class="approval-text">{request.request.describe("The agent")}</div>
            <div class="approval-actions">
                <button class="primary" on:click=move |_| answer(Decision::Once)>"Allow once"</button>
                <button class="ghost" on:click=move |_| answer(Decision::Session)>"Allow for this chat"</button>
                <button class="ghost danger" on:click=move |_| answer(Decision::Deny)>"Deny"</button>
            </div>
        </div>
    }
}

/// The agent's question: pick from its options, add words of your own.
#[component]
pub fn AskUser(chat: Chat, id: Option<String>) -> impl IntoView {
    let app = state();
    let chosen = RwSignal::new(Vec::<usize>::new());
    let own = RwSignal::new(String::new());
    move || {
        let question = chat.with(|c| c.question.clone())?;
        let multiple = question.multiple;
        let (for_answer, for_decline, id) = (question.clone(), question.clone(), id.clone());
        let id_decline = id.clone();
        let submit = move |_| {
            let reply = for_answer.answer(&chosen.get_untracked(), &own.get_untracked());
            app.send(id.clone(), reply, false);
            chat.update(|c| c.question = None);
            chosen.set(Vec::new());
            own.set(String::new());
        };
        let decline = move |_| {
            let reply = for_decline.declined(&own.get_untracked());
            app.send(id_decline.clone(), reply, false);
            chat.update(|c| c.question = None);
        };
        Some(view! {
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
                    <button class="ghost" on:click=decline>"Decline"</button>
                </div>
            </div>
        })
    }
}
