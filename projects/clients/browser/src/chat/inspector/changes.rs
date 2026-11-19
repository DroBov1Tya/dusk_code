//! The right column of a chat: its plan and tasks, subagents, changed files, background
//! jobs and what the context holds.

use crate::{api, pages::review::DiffView, state::state, ui::run};
use dusk_protocol::project::{Change, ChangeKind, FileDiff};
use leptos::prelude::*;

pub fn change_mark(kind: ChangeKind) -> &'static str {
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
pub fn ChangesTab(id: String) -> impl IntoView {
    let changes = RwSignal::new(Vec::<Change>::new());
    let diff = RwSignal::new(Vec::<FileDiff>::new());
    let thread = id.clone();
    let load = move || {
        let thread = thread.clone();
        run(
            async move { api::changed_files(&thread).await },
            move |list| changes.set(list),
        );
    };
    load();
    let show = {
        let id = id.clone();
        move |path: Option<String>| {
            let id = id.clone();
            run(
                async move { api::chat_diff(&id, path).await },
                move |files| diff.set(files),
            );
        }
    };
    let show_all = show.clone();
    // The project's pull requests, as the core keeps them told.
    let app = state();
    if let Some(project) = app.project.get_untracked() {
        let id = project.clone();
        run(
            async move { api::repository_status(&id).await },
            move |status| app.repository.set(Some((project, status))),
        );
    }
    let pulls = move || {
        let project = app.project.get()?;
        app.repository
            .get()
            .filter(|(of, _)| *of == project && !of.is_empty())
            .map(|(_, status)| status)
            .filter(|status| !status.pulls.is_empty())
    };
    view! {
        <div class="changes">
            {move || pulls().map(|status| view! {
                <ul class="pulls">
                    {status.pulls.iter().map(|pull| crate::pages::project::pull_row(&status.repository, pull)).collect_view()}
                </ul>
            })}
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
