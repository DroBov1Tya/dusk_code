//! Projects: adding and editing one, and its page with git, files and environment.

use super::review::DiffView;
use crate::{
    api,
    state::{View, state},
    ui::{Icon, IconName, Section, confirm, run},
};
use dusk_protocol::project::{
    CheckState, FileDiff, FileEntry, GitInfo, Project, PullState, PullStatus,
};
use leptos::prelude::*;

/// The dialog that adds a project (`Some(None)`) or edits one (`Some(Some(project))`).
#[component]
pub fn ProjectDialog(editing: RwSignal<Option<Option<Project>>>) -> impl IntoView {
    let app = state();
    let dialog = NodeRef::<leptos::html::Dialog>::new();
    let (name, description, dir) = (
        RwSignal::new(String::new()),
        RwSignal::new(String::new()),
        RwSignal::new(String::new()),
    );
    Effect::new(move |_| {
        if let Some(open) = editing.get() {
            name.set(open.as_ref().map(|p| p.name.clone()).unwrap_or_default());
            description.set(
                open.as_ref()
                    .map(|p| p.description.clone())
                    .unwrap_or_default(),
            );
            dir.set(
                open.as_ref()
                    .map(|p| p.dir.display().to_string())
                    .unwrap_or_default(),
            );
        }
    });
    Effect::new(move |_| {
        if let Some(element) = dialog.get()
            && element.is_connected()
            && !element.open()
            && element.show_modal().is_err()
        {
            editing.set(None);
            app.fail("Could not open the project editor.");
        }
    });
    let dismiss = move || {
        if let Some(element) = dialog.get_untracked() {
            element.close();
        }
        editing.set(None);
    };
    let save = move |_| {
        let id = editing.get_untracked().flatten().map(|p| p.id);
        let (n, d, p) = (
            name.get_untracked(),
            description.get_untracked(),
            dir.get_untracked(),
        );
        run(api::save_project(id, n, d, p), move |_| {
            dismiss();
            app.refresh();
        });
    };
    let remove = move |_| {
        let Some(project) = editing.get_untracked().flatten() else {
            return;
        };
        if confirm("Remove this project from duskcode? Its folder stays as it is.") {
            run(
                async move { api::remove_project(&project.id).await },
                move |()| {
                    dismiss();
                    app.refresh();
                },
            );
        }
    };
    view! {
        <Show when=move || editing.get().is_some()>
            <dialog class="project-dialog" node_ref=dialog aria-labelledby="project-dialog-title"
                on:click=move |_| dismiss()
                on:keydown=move |event| {
                    event.stop_propagation();
                    if event.key() == "Escape" {
                        event.prevent_default();
                        dismiss();
                    }
                }>
                <div class="dialog" on:click=|ev| ev.stop_propagation()>
                    <h3 id="project-dialog-title">{move || if editing.get().flatten().is_some() { "Edit project" } else { "Add a project" }}</h3>
                    <label>"Name"<input autofocus prop:value=move || name.get() on:input=move |ev| name.set(event_target_value(&ev)) /></label>
                    <label>"Folder on the machine running duskcode"
                        <input placeholder="/Users/me/code/app" prop:value=move || dir.get()
                            on:input=move |ev| dir.set(event_target_value(&ev)) /></label>
                    <label>"What it is, for the agents"
                        <textarea rows="3" prop:value=move || description.get()
                            on:input=move |ev| description.set(event_target_value(&ev))></textarea></label>
                    <div class="row-actions">
                        <button class="primary" on:click=save>"Save"</button>
                        <button class="ghost" on:click=move |_| dismiss()>"Cancel"</button>
                        <span class="grow"></span>
                        <Show when=move || editing.get().flatten().is_some()>
                            <button class="ghost danger" on:click=remove>"Remove"</button>
                        </Show>
                    </div>
                </div>
            </dialog>
        </Show>
    }
}

#[component]
pub fn ProjectPage(id: String) -> impl IntoView {
    let app = state();
    let Some(project) = app.project_by_id(&id) else {
        return view! { <p class="muted pad">"This project is gone."</p> }.into_any();
    };
    view! {
        <div class="page">
            <h2>{project.name.clone()}</h2>
            <p class="muted">{project.dir.display().to_string()}</p>
            <p>{project.description.clone()}</p>
            <button class="primary" on:click={let id = id.clone(); move |_| app.new_chat(Some(id.clone()))}>"New chat here"</button>
            <GitSection id=id.clone() />
            <FilesSection id=id.clone() />
            <EnvSection id />
        </div>
    }
    .into_any()
}

#[component]
fn GitSection(id: String) -> impl IntoView {
    let info = RwSignal::new(None::<GitInfo>);
    let diff = RwSignal::new(Vec::<FileDiff>::new());
    let load = {
        let id = id.clone();
        move || {
            let id = id.clone();
            let asked = id.clone();
            run(async move { api::git(&asked).await }, move |git| {
                info.set(Some(git))
            });
            let asked = id.clone();
            run(
                async move { api::repository_status(&asked).await },
                move |status| state().repository.set(Some((id, status))),
            );
        }
    };
    let pulls = {
        let id = id.clone();
        move || {
            state()
                .repository
                .get()
                .filter(|(shown, _)| *shown == id)
                .map(|(_, status)| status)
        }
    };
    load();
    let switch = {
        let (id, load) = (id.clone(), load.clone());
        move |ev| {
            let (id, branch, load) = (id.clone(), event_target_value(&ev), load.clone());
            run(
                async move { api::switch_branch(&id, branch).await },
                move |()| load(),
            );
        }
    };
    let show = {
        let id = id.clone();
        move |path: Option<String>| {
            let id = id.clone();
            run(
                async move { api::project_diff(&id, path).await },
                move |files| diff.set(files),
            );
        }
    };
    let show_all = show.clone();
    view! {
        <Section title="Git">
            {move || info.get().map(|git| {
                let current = git.branch.clone().unwrap_or_default();
                let (show, load) = (show.clone(), load.clone());
                view! {
                    <div class="row-actions">
                        <select prop:value=current.clone() on:change={let switch = switch.clone(); move |ev| switch(ev)}>
                            {git.branches.clone().into_iter().map(|b| view! {
                                <option value=b.clone() selected=b == current>{b.clone()}</option>
                            }).collect_view()}
                        </select>
                        {git.ahead_behind.map(|(a, b)| view! { <span class="muted">{format!("↑{a} ↓{b}")}</span> })}
                        <button class="ghost" on:click=move |_| load()>"Refresh"</button>
                        <button class="ghost" on:click={let show_all = show_all.clone(); move |_| show_all(None)}>"Review all"</button>
                    </div>
                    <ul class="files">{git.changes.clone().into_iter().map(|change| {
                        let (path, show) = (change.path.clone(), show.clone());
                        view! { <li><button class="file" on:click=move |_| show(Some(path.clone()))>{change.path.clone()}</button></li> }
                    }).collect_view()}</ul>
                }
            })}
            {move || pulls().map(|status| view! {
                <ul class="pulls">
                    {status.pulls.iter().map(|pull| pull_row(&status.repository, pull)).collect_view()}
                </ul>
                {status.unavailable.clone().map(|reason| view! { <p class="muted">{reason}</p> })}
            })}
            <DiffView files=diff />
        </Section>
    }
}

/// A pull request: its state, title, number and lines, and a dot for its checks. The
/// core gives https links only.
fn pull_row(repository: &str, pull: &PullStatus) -> impl IntoView + use<> {
    let (tone, state) = match pull.state {
        PullState::Open => ("tone-success", "open"),
        PullState::Draft => ("tone-neutral", "draft"),
        PullState::Merged => ("tone-accent", "merged"),
        PullState::Closed => ("tone-danger", "closed"),
    };
    let dot = pull.checks.state.map(|checks| match checks {
        CheckState::Passing => "dot on",
        CheckState::Pending => "dot wait running",
        CheckState::Failing => "dot off",
    });
    let checks = format!(
        "{} checks: {} passed, {} running, {} failed",
        pull.checks.total, pull.checks.passed, pull.checks.running, pull.checks.failed
    );
    let url = Some(pull.url.clone()).filter(|url| url.starts_with("https://"));
    view! {
        <li>
            <span class=format!("tone {tone} pull-state")>{state}</span>
            <a class="pull-title" href=url target="_blank" rel="noopener noreferrer">
                {pull.title.clone()}
            </a>
            <span class="muted">{format!("{repository} #{}", pull.number)}</span>
            <span class="add">{format!("+{}", pull.added)}</span>
            <span class="del">{format!("\u{2212}{}", pull.removed)}</span>
            {dot.map(|dot| view! { <span class=dot title=checks></span> })}
        </li>
    }
}

#[component]
fn FilesSection(id: String) -> impl IntoView {
    view! {
        <Section title="Files">
            <FolderList id path=String::new() />
        </Section>
    }
}

/// One folder of a project, which opens folder by folder.
#[component]
fn FolderList(id: String, path: String) -> impl IntoView {
    let entries = RwSignal::new(Vec::<FileEntry>::new());
    {
        let (id, path) = (id.clone(), path.clone());
        run(async move { api::files(&id, path).await }, move |list| {
            entries.set(list)
        });
    }
    view! {
        <ul class="tree">
            {move || entries.get().into_iter().map(|entry| {
                let name = entry.path.rsplit('/').next().unwrap_or(&entry.path).to_owned();
                if entry.is_dir {
                    let (id, folder) = (id.clone(), entry.path.clone());
                    let open = RwSignal::new(false);
                    view! {
                        <li>
                            <button class="file dir" on:click=move |_| open.update(|o| *o = !*o)>{format!("{name}/")}</button>
                            <Show when=move || open.get()>
                                <FolderList id=id.clone() path=folder.clone() />
                            </Show>
                        </li>
                    }.into_any()
                } else {
                    view! { <li><span class="file">{name}</span></li> }.into_any()
                }
            }).collect_view()}
        </ul>
    }
}

/// The variables a project's commands get; only their names are ever shown.
#[component]
fn EnvSection(id: String) -> impl IntoView {
    let names = RwSignal::new(Vec::<String>::new());
    let (name, value) = (RwSignal::new(String::new()), RwSignal::new(String::new()));
    {
        let id = id.clone();
        run(async move { api::env_names(&id).await }, move |list| {
            names.set(list)
        });
    }
    let add = {
        let id = id.clone();
        move |_| {
            let (id, n, v) = (id.clone(), name.get_untracked(), value.get_untracked());
            run(async move { api::set_env(&id, n, v).await }, move |list| {
                names.set(list);
                name.set(String::new());
                value.set(String::new());
            });
        }
    };
    view! {
        <Section title="Environment">
            <p class="muted">"Values stay in the core and are never shown here."</p>
            <ul class="files">
                {move || names.get().into_iter().map(|n| {
                    let (id, remove) = (id.clone(), n.clone());
                    view! { <li><code>{n}</code>
                        <button class="icon" aria-label=format!("Remove environment variable {remove}") on:click=move |_| {
                            let (id, remove) = (id.clone(), remove.clone());
                            run(async move { api::remove_env(&id, remove).await }, move |list| names.set(list));
                        }><Icon name=IconName::Close /></button></li> }
                }).collect_view()}
            </ul>
            <div class="row-actions">
                <input placeholder="NAME" prop:value=move || name.get() on:input=move |ev| name.set(event_target_value(&ev)) />
                <input type="password" autocomplete="off" placeholder="value" prop:value=move || value.get()
                    on:input=move |ev| value.set(event_target_value(&ev)) />
                <button class="ghost" on:click=add>"Set"</button>
            </div>
        </Section>
    }
}

pub fn open(id: String) {
    state().show(View::Project(id));
}
