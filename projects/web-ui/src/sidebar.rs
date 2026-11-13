//! The left column: new chat, search, projects with their chats, and the pages.

use crate::{
    api,
    link::Conn,
    pages::{connectors::Tab, project::ProjectDialog, settings::Page},
    state::{AppState, View, state},
    ui::run,
};
use dusk_protocol::{project::Project, thread::Thread};
use leptos::prelude::*;

/// The name a chat has in the list.
pub fn chat_title(thread: &Thread) -> String {
    thread
        .title
        .clone()
        .or_else(|| thread.name.clone())
        .unwrap_or_else(|| "New chat".to_owned())
}

#[component]
pub fn Sidebar() -> impl IntoView {
    let app = state();
    let search = RwSignal::new(String::new());
    let editing = RwSignal::new(None::<Option<Project>>);
    // Subagent threads belong to their parent chat, not to this list.
    let listed = Memo::new(move |_| {
        let query = search.get().to_lowercase();
        let mut threads: Vec<Thread> = app.threads.with(|threads| {
            threads
                .iter()
                .filter(|t| t.parent_thread_id.is_none())
                .filter(|t| query.is_empty() || chat_title(t).to_lowercase().contains(&query))
                .cloned()
                .collect()
        });
        threads.sort_by_key(|t| std::cmp::Reverse(t.active_at));
        threads
    });
    let loose = move || {
        let dirs: Vec<String> = app.projects.with(|projects| {
            projects
                .iter()
                .map(|p| p.dir.display().to_string())
                .collect()
        });
        listed
            .get()
            .into_iter()
            .filter(|t| t.project.as_ref().is_none_or(|dir| !dirs.contains(dir)))
            .collect::<Vec<_>>()
    };
    view! {
        <aside class="sidebar">
            <div class="brand">
                <span>"duskcode"</span>
                <span class=move || format!("dot {}", match app.conn.get() {
                    Conn::Online => "on",
                    Conn::Connecting => "wait",
                    Conn::Lost => "off",
                }) title="connection"></span>
            </div>
            <button class="primary wide" on:click=move |_| app.new_chat(app.project.get_untracked())>
                "New chat"
            </button>
            <button class="wide" title="Kept in memory only; gone when duskcode stops"
                on:click=move |_| app.new_temporary_chat()>
                "Temporary chat"
            </button>
            <input
                class="search"
                placeholder="Search chats"
                prop:value=move || search.get()
                on:input=move |ev| search.set(event_target_value(&ev))
            />
            <div class="scroll">
                <div class="group-head">
                    <span>"Projects"</span>
                    <button class="icon" title="Add a project" on:click=move |_| editing.set(Some(None))>"+"</button>
                </div>
                <For each=move || app.projects.get() key=|p| p.id.clone() let:project>
                    {project_group(app, project, listed, editing)}
                </For>
                <div class="group-head"><span>"Chats"</span></div>
                <For each=loose key=|t| t.id.clone() let:thread>
                    <ChatRow thread />
                </For>
            </div>
            <nav class="nav">
                <NavButton label="Connectors" target=View::Connectors(Tab::Agents) />
                <NavButton label="Scheduled tasks" target=View::Schedules />
                <NavButton label="Artifacts" target=View::Artifacts />
                <NavButton label="Settings" target=View::Settings(Page::Appearance) />
                <button class="nav-item" on:click=move |_| leptos::task::spawn_local(async {
                    crate::session::sign_out().await;
                    if let Some(window) = web_sys::window() {
                        let _ = window.location().reload();
                    }
                })>"Sign out"</button>
                {move || app.status.with(|s| s.as_ref().and_then(|s| s.available_update.clone())).map(|tag| view! {
                    <button class="update" on:click=move |_| app.view.set(View::Settings(Page::Updates))>
                        {format!("Update {tag}")}
                    </button>
                })}
            </nav>
            <ProjectDialog editing />
        </aside>
    }
}

fn project_group(
    app: AppState,
    project: Project,
    listed: Memo<Vec<Thread>>,
    editing: RwSignal<Option<Option<Project>>>,
) -> impl IntoView {
    let dir = project.dir.display().to_string();
    let id = project.id.clone();
    let selected = {
        let id = id.clone();
        move || app.project.get().as_deref() == Some(id.as_str())
    };
    let chats = move || {
        listed
            .get()
            .into_iter()
            .filter(|t| t.project.as_deref() == Some(dir.as_str()))
            .collect::<Vec<_>>()
    };
    let (new_id, edit_project) = (id.clone(), project.clone());
    view! {
        <div class="project" class:selected=selected>
            <div class="project-head">
                <button class="project-name" title=project.description.clone()
                    on:click=move |_| { app.project.set(Some(id.clone())); crate::pages::project::open(id.clone()); }>
                    {project.name.clone()}
                </button>
                <button class="icon" title="New chat here"
                    on:click=move |_| app.new_chat(Some(new_id.clone()))>"+"</button>
                <button class="icon" title="Edit"
                    on:click=move |_| editing.set(Some(Some(edit_project.clone())))>"…"</button>
            </div>
            <For each=chats key=|t| t.id.clone() let:thread>
                <ChatRow thread />
            </For>
        </div>
    }
}

#[component]
fn ChatRow(thread: Thread) -> impl IntoView {
    let app = state();
    let id = thread.id.clone();
    let active = {
        let id = id.clone();
        move || app.view.get() == View::Chat(Some(id.clone()))
    };
    let working = {
        let id = id.clone();
        move || app.chat(&id).is_some_and(|chat| chat.with(|c| c.working))
    };
    let title = chat_title(&thread);
    let open_id = id.clone();
    view! {
        <div class="chat-row" class:active=active>
            <button class="chat-title" on:click=move |_| app.open_chat(open_id.clone())>
                {title}
            </button>
            <Show when=working><span class="spinner" title="working"></span></Show>
            <button class="icon hover" title="Delete chat" on:click=move |_| {
                let id = id.clone();
                if confirm("Delete this chat for good?") {
                    let doomed = id.clone();
                    run(async move { api::delete_thread(&doomed).await }, move |()| {
                        app.chats.update(|chats| { chats.remove(&id); });
                        app.refresh();
                        if app.view.get_untracked() == View::Chat(Some(id.clone())) {
                            app.view.set(View::Chat(None));
                        }
                    });
                }
            }>"×"</button>
        </div>
    }
}

#[component]
fn NavButton(label: &'static str, target: View) -> impl IntoView {
    let app = state();
    let same = {
        let target = target.clone();
        move || std::mem::discriminant(&app.view.get()) == std::mem::discriminant(&target)
    };
    view! {
        <button class="nav-item" class:active=same on:click=move |_| app.view.set(target.clone())>
            {label}
        </button>
    }
}

/// A native yes/no question; the page has nothing else to ask it with.
pub fn confirm(question: &str) -> bool {
    web_sys::window()
        .and_then(|window| window.confirm_with_message(question).ok())
        .unwrap_or(false)
}
