//! The right column of a chat: its plan and tasks, subagents, changed files, background
//! jobs and what the context holds.

use crate::{api, ui::run};
use dusk_protocol::thread::JobInfo;
use leptos::prelude::*;

#[component]
pub fn JobsTab(id: String) -> impl IntoView {
    let jobs = RwSignal::new(Vec::<JobInfo>::new());
    let output = RwSignal::new(None::<(String, String)>);
    let thread = id.clone();
    let reload = move || {
        let thread = thread.clone();
        run(async move { api::jobs(&thread).await }, move |list| {
            jobs.set(list)
        });
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
