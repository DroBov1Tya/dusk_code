//! The plan strip above the composer, as the desktop app has it: one line folded, with
//! the plan's counts from the core, its bar and the task in work; unfolded, the numbered
//! rows with their status words in one column at the right. A task that waits for the
//! user has `confirm` for its word, and a click passes it.

use crate::{
    api,
    state::Chat,
    ui::{Icon, IconName, run, tone_text},
};
use dusk_protocol::thread::{
    GoalCheck, PlanCounts, Task, TaskCard, TaskStatus, TeamEnd, WorkKind, task_card, task_in_work,
};
use leptos::prelude::*;

#[component]
pub fn PlanStrip(id: String, chat: Chat) -> impl IntoView {
    let open = RwSignal::new(false);
    // The task whose card shows under the rows.
    let chosen = RwSignal::new(None::<String>);
    let counts = RwSignal::new(None::<PlanCounts>);
    let tasks = Memo::new(move |_| chat.with(|c| c.tasks.clone()));
    // The counts and the team are the core's: asked again whenever the tasks move. The
    // team's own events keep it current between.
    let asked = id.clone();
    Effect::new(move |_| {
        tasks.track();
        let id = asked.clone();
        run(
            async move { Ok((api::plan_counts(&id).await?, api::team(&id).await?)) },
            move |(c, team)| {
                counts.set(Some(c));
                if let Some(team) = team {
                    chat.update(|chat| chat.set_team(team, super::data::now_ms()));
                }
            },
        );
    });
    let shown = move || !tasks.with(Vec::is_empty) || counts.get().is_some_and(|c| c.total() > 0);
    // An ended team says how it ended in the place of a task in work: none is.
    let current = move || {
        chat.with(
            |c| match c.team.as_ref().and_then(|team| Some((team.ended()?, team))) {
                Some((end, team)) => {
                    let tone = if end == TeamEnd::Done {
                        "tone-good"
                    } else {
                        "tone-warn"
                    };
                    (team.progress_note(), tone)
                }
                None => (
                    task_in_work(&c.tasks, c.team.as_ref())
                        .map(|(n, t)| t.numbered(n))
                        .unwrap_or_default(),
                    "muted",
                ),
            },
        )
    };
    let id = StoredValue::new(id);
    // While the plan is written, its label carries the light.
    let planning = Memo::new(move |_| {
        chat.with(|c| {
            c.work(super::data::now_ms())
                .is_some_and(|work| work.kind == WorkKind::Planning)
        })
    });
    // A row is drawn once for its task and then follows it, so only a task that just
    // came draws in.
    let numbers = Memo::new(move |_| {
        tasks.with(|tasks| {
            tasks
                .iter()
                .enumerate()
                .map(|(i, task)| (i + 1, task.task_id.clone()))
                .collect::<Vec<_>>()
        })
    });
    view! {
        <Show when=shown>
            <div class="plan-strip">
                <button class="plan-line" aria-expanded=move || open.get().to_string()
                    on:click=move |_| open.update(|o| *o = !*o)>
                    <Icon name=IconName::ChevronDown />
                    <span class="plan-label" class:act-shimmer=move || planning.get()>"Plan"</span>
                    {move || match counts.get() {
                        Some(c) => view! { <span class="plan-summary">{c.summary()}</span> }.into_any(),
                        None => view! { <span class="plan-placeholder"></span> }.into_any(),
                    }}
                    {move || {
                        let (text, tone) = current();
                        let ended = chat.with(|c| c.team.as_ref().is_some_and(|t| t.ended().is_some()));
                        view! { <span class=format!("plan-current {tone}") class:ended=ended>{text}</span> }
                    }}
                    <Show when=move || counts.get().is_some()>{bar(counts)}</Show>
                </button>
                <Show when=move || open.get()>
                    <ol class="plan-rows">
                        <For each=move || numbers.get() key=|(_, task_id)| task_id.clone() let:numbered>
                            {
                                let (first_number, task_id) = numbered;
                                let found = task_id.clone();
                                let task = Memo::new(move |_| {
                                    tasks.with(|tasks| {
                                        tasks
                                            .iter()
                                            .enumerate()
                                            .find(|(_, task)| task.task_id == found)
                                            .map(|(i, task)| (i + 1, task.clone()))
                                    })
                                });
                                let _ = first_number;
                                let (found_row, clicked) = (task_id.clone(), task_id);
                                view! {
                                    <li class="plan-row" class:done=move || task.with(|t| t.as_ref().is_some_and(|(_, t)| t.status == TaskStatus::Done))
                                        class:chosen=move || chosen.with(|c| c.as_deref() == Some(found_row.as_str()))
                                        on:click=move |_| chosen.update(|c| *c = match c.as_deref() == Some(clicked.as_str()) {
                                            true => None,
                                            false => Some(clicked.clone()),
                                        })>
                                        {move || task.get().map(|(number, task)| row(number, task, id.get_value()))}
                                    </li>
                                }
                            }
                        </For>
                    </ol>
                    {move || chosen.get().and_then(|task_id| {
                        let card = chat.with(|c| task_card(&c.tasks, &task_id))?;
                        Some(card_view(card, id.get_value(), chosen))
                    })}
                </Show>
            </div>
        </Show>
    }
}

fn row(number: usize, task: Task, thread_id: String) -> impl IntoView {
    let (glyph, tone) = (task.status.glyph(), tone_text(task.status.tone()));
    let for_user =
        task.status == TaskStatus::Review && task.gate == Some(GoalCheck::UserConfirmation);
    let word = if for_user {
        let task_id = task.task_id.clone();
        view! {
            <button class="link plan-word" title="Confirm this task is done" on:click=move |ev| {
                // The row under it opens the card; this click only confirms.
                ev.stop_propagation();
                let (thread_id, task_id) = (thread_id.clone(), task_id.clone());
                run(async move { api::confirm_task(&thread_id, &task_id).await }, |()| ());
            }>"confirm"</button>
        }
        .into_any()
    } else {
        view! { <span class=format!("plan-word {tone}")>{task.status.name()}</span> }.into_any()
    };
    view! {
        <span class="plan-number muted num">{number}</span>
        <span class=format!("plan-glyph {tone}") aria-hidden="true">{glyph}</span>
        <span class="plan-text">{task.title()}</span>
        {task.owner.map(|owner| view! { <span class="plan-owner muted">{owner.name}</span> })}
        {word}
    }
}

/// A task's card under the rows: why it waits or failed, what it depends on and spent,
/// and its member's chat a click away. The fields are the protocol's.
fn card_view(card: TaskCard, thread_id: String, chosen: RwSignal<Option<String>>) -> AnyView {
    let app = crate::state::state();
    let tone = tone_text(card.status.tone());
    let fields = card
        .fields
        .into_iter()
        .map(|(label, value)| {
            view! { <div class="plan-field"><span class="muted">{label}</span><span>{value}</span></div> }
        })
        .collect_view();
    let owner = card.owner.map(|owner| {
        let open = (thread_id, owner.thread_id);
        view! {
            <button class="ghost" on:click=move |_| app.subagent.set(Some(open.clone()))>
                {format!("Open {}'s chat", owner.name)}
            </button>
        }
    });
    view! {
        <div class="plan-card">
            <div class="plan-card-head">
                <b>{card.title}</b>
                <span class="grow"></span>
                <button class="ghost" on:click=move |_| chosen.set(None)>"Close"</button>
            </div>
            <div class=tone>{card.head}</div>
            {fields}
            <div class="plan-card-foot">{owner}</div>
        </div>
    }
    .into_any()
}

/// The plan as one bar of fixed width, the shares in their colours. The four shares
/// stay in the page and only their widths change, so the bar eases to new shares.
fn bar(counts: RwSignal<Option<PlanCounts>>) -> impl IntoView {
    type Share = (fn(PlanCounts) -> u32, &'static str);
    let parts: [Share; 4] = [
        (|c| c.done, "good"),
        (|c| c.running, "accent"),
        (|c| c.failed, "bad"),
        (|c| c.blocked, "warn"),
    ];
    view! {
        <span class="plan-bar" aria-hidden="true">
            {parts.into_iter().map(|(of, tone)| view! {
                <span class=format!("tone-{tone}") style:width=move || {
                    let counts = counts.get().unwrap_or_default();
                    format!("{}%", of(counts) as f32 * 100. / counts.total().max(1) as f32)
                }></span>
            }).collect_view()}
        </span>
    }
}
