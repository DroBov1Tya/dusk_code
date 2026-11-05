//! The window's one root view: what it knows of the core, which tab each column shows,
//! and the three columns themselves. Each column's module adds its own methods here.

use std::collections::{BTreeMap, HashMap, HashSet};

use dusk_protocol::{
    client::{Event, Status},
    context::ContextReport,
    project::{FileEntry, GitInfo, Project},
    providers::ProviderStatus,
    settings::Settings,
    thread::Thread,
    work::JobInfo,
};
use gpui_kit::{
    component::{
        ActiveTheme,
        input::{InputEvent, InputState, TextareaState},
    },
    *,
};
use tokio::sync::{mpsc, watch};

use crate::{chat, chat::transcript::Transcript, core::Core, inspector, settings, sidebar};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Center {
    Chat,
    Settings,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Left {
    Chats,
    Connectors,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Right {
    Git,
    Files,
    Context,
    Todo,
    Agents,
    Jobs,
}

/// The form that adds a project, or changes the one with `id`.
pub struct ProjectForm {
    pub id: Option<String>,
    pub name: Entity<InputState>,
    pub description: Entity<InputState>,
    pub dir: Entity<InputState>,
}

/// The model menu over the composer, and which of its submenus is open.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Picker {
    Model,
    Effort,
    MoreModels,
}

pub struct Workspace {
    pub core: Core,
    pub status: Status,
    pub projects: Vec<Project>,
    /// Every chat the core keeps; subagent threads are left out.
    pub threads: Vec<Thread>,
    /// Chats opened in this window, by thread ID.
    pub chats: HashMap<String, Transcript>,
    pub shown: Option<String>,
    pub project: Option<String>,
    pub collapsed: HashSet<String>,
    pub center: Center,
    pub left: Left,
    pub right: Right,
    pub search: Entity<InputState>,
    pub composer: Entity<TextareaState>,
    pub form: Option<ProjectForm>,
    /// The last error, shown until it is dismissed.
    pub notice: Option<String>,
    pub git: Option<GitInfo>,
    /// Folder entries by folder, `""` for the project's top.
    pub files: BTreeMap<String, Vec<FileEntry>>,
    pub expanded: HashSet<String>,
    pub context: Option<ContextReport>,
    pub jobs: Vec<JobInfo>,
    pub settings: Option<Settings>,
    pub providers: Vec<ProviderStatus>,
    pub picker: Option<Picker>,
    /// Answers whose reasoning is unfolded, by chat and item index.
    pub unfolded: HashSet<(String, usize)>,
    pub scroll: ScrollHandle,
    /// The transcript size last scrolled to, so the view follows only new output.
    pub followed: (usize, bool),
}

impl Workspace {
    pub fn new(
        core: Core,
        events: mpsc::UnboundedReceiver<Event>,
        status: watch::Receiver<Status>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search chats"));
        cx.subscribe_in(&search, window, |_, _, _: &InputEvent, _, cx| cx.notify())
            .detach();
        let composer = cx.new(|cx| {
            let mut state = TextareaState::new(window, cx)
                .auto_grow(2, 10)
                .placeholder("Ask anything. Enter sends, Shift+Enter starts a new line.");
            state.set_submit_on_enter(true, cx);
            state
        });
        cx.subscribe_in(&composer, window, |this, _, event: &InputEvent, window, cx| {
            if let InputEvent::PressEnter { shift: false, .. } = event {
                this.send(window, cx);
            }
        })
        .detach();
        let first = status.borrow().clone();
        let mut workspace = Self {
            core,
            status: first,
            projects: Vec::new(),
            threads: Vec::new(),
            chats: HashMap::new(),
            shown: None,
            project: None,
            collapsed: HashSet::new(),
            center: Center::Chat,
            left: Left::Chats,
            right: Right::Git,
            search,
            composer,
            form: None,
            notice: None,
            git: None,
            files: BTreeMap::new(),
            expanded: HashSet::new(),
            context: None,
            jobs: Vec::new(),
            settings: None,
            providers: Vec::new(),
            picker: None,
            unfolded: HashSet::new(),
            scroll: ScrollHandle::new(),
            followed: (0, false),
        };
        workspace.pump_events(events, cx);
        workspace.pump_status(status, cx);
        workspace.reload_projects(cx);
        workspace.reload_threads(cx);
        workspace
    }

    fn pump_events(&mut self, mut events: mpsc::UnboundedReceiver<Event>, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            while let Some(event) = events.recv().await {
                let alive = this.update(cx, |this, cx| {
                    this.on_event(event, cx);
                    cx.notify();
                });
                if alive.is_err() {
                    return;
                }
            }
        })
        .detach();
    }

    fn pump_status(&mut self, mut status: watch::Receiver<Status>, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            while status.changed().await.is_ok() {
                let next = status.borrow_and_update().clone();
                let alive = this.update(cx, |this, cx| {
                    this.status = next;
                    cx.notify();
                });
                if alive.is_err() {
                    return;
                }
            }
        })
        .detach();
    }

    fn on_event(&mut self, event: Event, cx: &mut Context<Self>) {
        match event {
            Event::Thread { thread_id, event } => {
                let titled = matches!(*event, dusk_protocol::thread::ThreadEvent::TitleChanged { .. });
                if let Some(chat) = self.chats.get_mut(&thread_id) {
                    chat.apply(*event);
                }
                if titled {
                    self.reload_threads(cx);
                }
            }
            Event::Ended { thread_id } => {
                if let Some(chat) = self.chats.get_mut(&thread_id) {
                    chat.working = false;
                }
            }
        }
    }

    /// Shows an error until it is dismissed; also kept in the log.
    pub fn fail(&mut self, error: anyhow::Error, cx: &mut Context<Self>) {
        tracing::warn!("{error:#}");
        self.notice = Some(format!("{error:#}"));
        cx.notify();
    }

    pub fn reload_threads(&mut self, cx: &mut Context<Self>) {
        self.core.call(
            cx,
            |client| async move { client.threads().list().await },
            |this, result, cx| match result {
                Ok(mut threads) => {
                    threads.retain(|thread| thread.parent_thread_id.is_none());
                    threads.sort_by_key(|thread| std::cmp::Reverse(thread.active_at));
                    this.threads = threads;
                    cx.notify();
                }
                Err(error) => this.fail(error, cx),
            },
        );
    }

    pub fn selected_project(&self) -> Option<&Project> {
        let id = self.project.as_deref()?;
        self.projects.iter().find(|project| project.id == id)
    }

    pub fn shown_chat(&self) -> Option<&Transcript> {
        self.chats.get(self.shown.as_deref()?)
    }
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (background, foreground) = (theme.background, theme.foreground);
        let center = match self.center {
            Center::Chat => chat::render(self, window, cx).into_any_element(),
            Center::Settings => settings::render(self, window, cx).into_any_element(),
        };
        div()
            .size_full()
            .flex()
            .flex_row()
            .bg(background)
            .text_color(foreground)
            .text_sm()
            .child(sidebar::render(self, window, cx))
            .child(div().flex_1().min_w_0().h_full().child(center))
            .child(inspector::render(self, window, cx))
    }
}
