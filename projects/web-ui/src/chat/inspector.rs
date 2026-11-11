//! The right column of a chat: its plan and tasks, subagents, changed files, background
//! jobs and what the context holds.

use super::{data::AgentState, subagent::state_text};
use crate::{
    api,
    markdown::Markdown,
    pages::review::DiffView,
    state::{View, state},
    ui::{compact, run},
};
use dusk_protocol::{project::{Change, ChangeKind, FileDiff}, thread::{ContextReport, JobInfo}};
use leptos::prelude::*;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Plan,
    Tasks,
    Agents,
    Changes,
    Jobs,
    Context,
}

const TABS: [(Tab, &str); 6] = [
    (Tab::Plan, "Plan"),
    (Tab::Tasks, "Tasks"),
    (Tab::Agents, "Agents"),
    (Tab::Changes, "Changes"),
    (Tab::Jobs, "Jobs"),
    (Tab::Context, "Context"),
];

#[component]
pub fn Inspector() -> impl IntoView {
    let app = state();
    let tab = RwSignal::new(Tab::Tasks);
    // Only an open chat has anything to inspect.
    let thread = move || match app.view.get() {
        View::Chat(Some(id)) => Some(id),
        _ => None,
    };
    move || {
        let id = thread()?;
        Some(view! {
            <aside class="inspector">
                <div class="tabs">
                    {TABS.map(|(t, label)| view! {
                        <button class="tab" class:active=move || tab.get() == t on:click=move |_| tab.set(t)>
                            {label}
                        </button>
                    })}
                </div>
                <div class="tab-body">
                    {let id = id.clone(); move || match tab.get() {
                        Tab::Plan => view! { <PlanTab id=id.clone() /> }.into_any(),
                        Tab::Tasks => view! { <TasksTab id=id.clone() /> }.into_any(),
                        Tab::Agents => view! { <AgentsTab id=id.clone() /> }.into_any(),
                        Tab::Changes => view! { <ChangesTab id=id.clone() /> }.into_any(),
                        Tab::Jobs => view! { <JobsTab id=id.clone() /> }.into_any(),
                        Tab::Context => view! { <ContextTab id=id.clone() /> }.into_any(),
                    }}
                </div>
            </aside>
        })
    }
}

#[component]
fn PlanTab(id: String) -> impl IntoView {
    let plan = RwSignal::new(String::new());
    let editing = RwSignal::new(false);
    let draft = RwSignal::new(String::new());
    let thread = id.clone();
    run(async move { api::plan(&thread).await }, move |text| plan.set(text.unwrap_or_default()));
    let save_id = id.clone();
    view! {
        <div class="plan">
            <Show when=move || !editing.get()
                fallback=move || view! {
                    <textarea class="plan-edit" prop:value=move || draft.get()
                        on:input=move |ev| draft.set(event_target_value(&ev))></textarea>
                    <div class="row-actions">
                        <button class="primary" on:click={
                            let id = save_id.clone();
                            move |_| {
                                let text = draft.get_untracked();
                                let id = id.clone();
                                run(async move { api::save_plan(&id, text.clone()).await.map(|()| text) },
                                    move |text| { plan.set(text); editing.set(false); });
                            }
                        }>"Save"</button>
                        <button class="ghost" on:click=move |_| editing.set(false)>"Cancel"</button>
                    </div>
                }>
                <Show when=move || !plan.get().is_empty()
                    fallback=|| view! { <p class="muted">"No plan yet. The agent pins one when the work needs it."</p> }>
                    <Markdown text=plan />
                </Show>
                <button class="ghost" on:click=move |_| { draft.set(plan.get_untracked()); editing.set(true); }>"Edit"</button>
            </Show>
        </div>
    }
}

#[component]
fn TasksTab(id: String) -> impl IntoView {
    let app = state();
    let tasks = Signal::derive(move || app.chat(&id).map(|c| c.with(|c| c.tasks.clone())).unwrap_or_default());
    view! {
        <div class="tasks">
            <Show when=move || !tasks.get().is_empty() fallback=|| view! { <p class="muted">"No tasks."</p> }>
                {move || tasks.get().into_iter().map(|task| view! {
                    <div class="task" class:done=task.done>
                        <div class="task-name">{task.description.clone()}</div>
                        <ul>{task.items.iter().map(|item| view! {
                            <li class:done=item.done>{item.description.clone()}</li>
                        }).collect_view()}</ul>
                    </div>
                }).collect_view()}
            </Show>
        </div>
    }
}

#[component]
fn AgentsTab(id: String) -> impl IntoView {
    let app = state();
    let thread = id.clone();
    let agents = Signal::derive(move || app.chat(&thread).map(|c| c.with(|c| c.agents.clone())).unwrap_or_default());
    view! {
        <div class="agents">
            <Show when=move || !agents.get().is_empty() fallback=|| view! { <p class="muted">"No subagents in this chat."</p> }>
                {let id = id.clone(); move || agents.get().into_iter().map(|agent| {
                    let (parent, thread_id) = (id.clone(), agent.thread_id.clone());
                    let working = agent.state.working();
                    view! {
                        <button class="agent" on:click=move |_| app.subagent.set(Some((parent.clone(), thread_id.clone())))>
                            <FoxBadge working />
                            <div class="agent-text">
                                <div class="agent-name">{agent.name.clone()}<span class="muted">{format!(" · {}", agent.role)}</span></div>
                                <div class="muted">{agent.task.clone()}</div>
                                <div class=agent_class(&agent.state)>{state_text(&agent.state)}</div>
                            </div>
                        </button>
                    }
                }).collect_view()}
            </Show>
        </div>
    }
}

fn agent_class(state: &AgentState) -> &'static str {
    match state {
        AgentState::Failed => "agent-state bad",
        AgentState::Done => "agent-state",
        _ => "agent-state live",
    }
}

/// The fox on a subagent's tile; its eyes open while the agent works.
#[component]
fn FoxBadge(working: bool) -> impl IntoView {
    view! {
        <svg class="fox" class:awake=working viewBox="0 0 32 32" aria-hidden="true">
            <path d="M3 4l8 5h10l8-5-2 13-9 11h-4L5 17z" class="fox-face" />
            <path d="M12 28l4-7 4 7z" class="fox-nose" />
            <circle cx="11.5" cy="16" r="1.6" class="fox-eye" />
            <circle cx="20.5" cy="16" r="1.6" class="fox-eye" />
        </svg>
    }
}

fn change_mark(kind: ChangeKind) -> &'static str {
    match kind {
        ChangeKind::Added => "A",
        ChangeKind::Modified => "M",
        ChangeKind::Deleted => "D",
        ChangeKind::Renamed => "R",
        ChangeKind::Untracked => "?",
        ChangeKind::Conflicted => "!",
    }
}

#[component]
fn ChangesTab(id: String) -> impl IntoView {
    let changes = RwSignal::new(Vec::<Change>::new());
    let diff = RwSignal::new(Vec::<FileDiff>::new());
    let thread = id.clone();
    let load = move || {
        let thread = thread.clone();
        run(async move { api::changed_files(&thread).await }, move |list| changes.set(list));
    };
    load();
    let show = {
        let id = id.clone();
        move |path: Option<String>| {
            let id = id.clone();
            run(async move { api::chat_diff(&id, path).await }, move |files| diff.set(files));
        }
    };
    let show_all = show.clone();
    view! {
        <div class="changes">
            <div class="row-actions">
                <button class="ghost" on:click=move |_| load()>"Refresh"</button>
                <button class="ghost" on:click=move |_| show_all(None)>"All changes"</button>
            </div>
            <ul class="files">
                {move || changes.get().into_iter().map(|change| {
                    let path = change.path.clone();
                    let show = show.clone();
                    view! {
                        <li><button class="file" on:click=move |_| show(Some(path.clone()))>
                            <span class="mark">{change_mark(change.kind)}</span>{change.path.clone()}
                        </button></li>
                    }
                }).collect_view()}
            </ul>
            <DiffView files=diff />
        </div>
    }
}

#[component]
fn JobsTab(id: String) -> impl IntoView {
    let jobs = RwSignal::new(Vec::<JobInfo>::new());
    let output = RwSignal::new(None::<(String, String)>);
    let thread = id.clone();
    let reload = move || {
        let thread = thread.clone();
        run(async move { api::jobs(&thread).await }, move |list| jobs.set(list));
    };
    reload();
    view! {
        <div class="jobs">
            <button class="ghost" on:click={let reload = reload.clone(); move |_| reload()}>"Refresh"</button>
            <Show when=move || !jobs.get().is_empty() fallback=|| view! { <p class="muted">"No background jobs."</p> }>
                {let id = id.clone(); let reload = reload.clone(); move || jobs.get().into_iter().map(|job| {
                    let (thread, job_id) = (id.clone(), job.job_id.clone());
                    let (stop_thread, stop_job, reload) = (id.clone(), job.job_id.clone(), reload.clone());
                    view! {
                        <div class="job">
                            <code>{job.command.clone()}</code>
                            <div class="muted">{job.status.clone()}</div>
                            <div class="row-actions">
                                <button class="ghost" on:click=move |_| {
                                    let (thread, job_id) = (thread.clone(), job_id.clone());
                                    run(async move { api::job_output(&thread, &job_id).await.map(|t| (job_id, t)) },
                                        move |shown| output.set(Some(shown)));
                                }>"Output"</button>
                                {job.stoppable.then(|| view! {
                                    <button class="ghost danger" on:click=move |_| {
                                        let (thread, job_id, reload) = (stop_thread.clone(), stop_job.clone(), reload.clone());
                                        run(async move { api::stop_job(&thread, &job_id).await }, move |_| reload());
                                    }>"Stop"</button>
                                })}
                            </div>
                        </div>
                    }
                }).collect_view()}
            </Show>
            {move || output.get().map(|(job, text)| view! {
                <div class="muted">{job}</div>
                <pre class="tool-out">{text}</pre>
            })}
        </div>
    }
}

#[component]
fn ContextTab(id: String) -> impl IntoView {
    let report = RwSignal::new(None::<ContextReport>);
    let load = move || {
        let id = id.clone();
        run(async move { api::context_report(Some(id)).await }, move |r| report.set(Some(r)));
    };
    load();
    view! {
        <div class="context-tab">
            <button class="ghost" on:click=move |_| load()>"Refresh"</button>
            {move || report.get().map(|report| view! {
                <p>{format!("{} of {} tokens", compact(report.total), compact(report.limit))}</p>
                {report.sections.iter().map(|section| view! {
                    <details class="section-row">
                        <summary><span>{section.name.clone()}</span><span class="muted">{compact(section.tokens)}</span></summary>
                        <ul>{section.items.iter().map(|(name, tokens)| view! {
                            <li><span>{name.clone()}</span><span class="muted">{compact(*tokens)}</span></li>
                        }).collect_view()}</ul>
                    </details>
                }).collect_view()}
            })}
        </div>
    }
}
