//! The plan strip above the composer, as the desktop app has it: one line folded, with
//! the plan's counts from the core, its bar and the task in work; unfolded, the numbered
//! rows with their status words in one column at the right. A task that waits for the
//! user has `confirm` for its word, and a click passes it.

use crate::{
    api,
    state::Chat,
    ui::{Icon, IconName, run},
};
use dusk_protocol::thread::{GoalCheck, PlanCounts, Task, TaskStatus, WorkKind};
use leptos::prelude::*;

#[component]
pub fn PlanStrip(id: String, chat: Chat) -> impl IntoView {
    let open = RwSignal::new(false);
    let counts = RwSignal::new(None::<PlanCounts>);
    let tasks = Memo::new(move |_| chat.with(|c| c.tasks.clone()));
    // The counts are the core's: asked again whenever the tasks move.
    let asked = id.clone();
    Effect::new(move |_| {
        tasks.track();
        let id = asked.clone();
        run(async move { api::plan_counts(&id).await }, move |c| {
            counts.set(Some(c))
        });
    });
    let shown = move || !tasks.with(Vec::is_empty) || counts.get().is_some_and(|c| total(c) > 0);
    let current = move || {
        tasks
            .with(|tasks| current(tasks).map(|(n, t)| format!("#{n} {}", one_line(&t.description))))
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
                        Some(c) => view! { <span class="plan-summary">{summary(c)}</span> }.into_any(),
                        None => view! { <span class="plan-placeholder"></span> }.into_any(),
                    }}
                    <span class="plan-current muted">{current}</span>
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
                                let _ = (first_number, task_id);
                                view! {
                                    <li class="plan-row" class:done=move || task.with(|t| t.as_ref().is_some_and(|(_, t)| t.status == TaskStatus::Done))>
                                        {move || task.get().map(|(number, task)| row(number, task, id.get_value()))}
                                    </li>
                                }
                            }
                        </For>
                    </ol>
                </Show>
            </div>
        </Show>
    }
}

fn row(number: usize, task: Task, thread_id: String) -> impl IntoView {
    let (glyph, tone) = look(task.status);
    let for_user =
        task.status == TaskStatus::Review && task.gate == Some(GoalCheck::UserConfirmation);
    let word = if for_user {
        let task_id = task.task_id.clone();
        view! {
            <button class="link plan-word" title="Confirm this task is done" on:click=move |_| {
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
        <span class="plan-text">{one_line(&task.description)}</span>
        {task.owner.map(|owner| view! { <span class="plan-owner muted">{owner.name}</span> })}
        {word}
    }
}

/// "3 of 7 done · 1 running": the parts that are not zero after the done.
fn summary(counts: PlanCounts) -> String {
    let mut text = format!("{} of {} done", counts.done, total(counts));
    for (count, word) in [
        (counts.running, "running"),
        (counts.claimed, "claimed"),
        (counts.review, "in review"),
        (counts.blocked, "blocked"),
        (counts.failed, "failed"),
    ] {
        if count > 0 {
            text.push_str(&format!(" \u{00B7} {count} {word}"));
        }
    }
    text
}

fn total(c: PlanCounts) -> u32 {
    [
        c.open, c.claimed, c.running, c.blocked, c.review, c.done, c.failed,
    ]
    .into_iter()
    .fold(0, u32::saturating_add)
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
                    format!("{}%", of(counts) as f32 * 100. / total(counts).max(1) as f32)
                }></span>
            }).collect_view()}
        </span>
    }
}

/// The task the folded line names: the first running, else the first not over.
fn current(tasks: &[Task]) -> Option<(usize, &Task)> {
    let numbered = || tasks.iter().enumerate().map(|(i, t)| (i + 1, t));
    numbered()
        .find(|(_, t)| t.status == TaskStatus::Running)
        .or_else(|| {
            numbered().find(|(_, t)| !matches!(t.status, TaskStatus::Done | TaskStatus::Failed))
        })
}

/// The glyph and tone of a status; its word is always beside it.
fn look(status: TaskStatus) -> (&'static str, &'static str) {
    match status {
        TaskStatus::Open => ("\u{25CB}", "tone-muted"),
        TaskStatus::Claimed => ("\u{25C9}", "tone-info"),
        TaskStatus::Running => ("\u{25CF}", "tone-accent"),
        TaskStatus::Blocked => ("\u{25C6}", "tone-warn"),
        TaskStatus::Review => ("\u{25C7}", "tone-info"),
        TaskStatus::Done => ("\u{2713}", "tone-good"),
        TaskStatus::Failed => ("\u{2715}", "tone-bad"),
    }
}

/// A task's words are model text: one line, drawn as plain text.
fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}
