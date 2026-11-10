//! The window's one root view: what it knows of the core, which tab each column shows,
//! and the three columns themselves. Each column's module adds its own methods here.

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::PathBuf,
};

use dusk_protocol::{
    client::{Event, Status},
    context::ContextReport,
    project::{FileEntry, GitInfo, OutputFile, OutputKind, Project},
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
    prelude::FluentBuilder,
    *,
};
use tokio::sync::{mpsc, watch};

use crate::{
    artifacts, chat, chat::transcript::Transcript, connectors, connectors::Shelf, core::Core,
    inspector, modal, modal::Modal, settings, settings::SettingsPage, sidebar,
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Center {
    Chat,
    Artifacts,
    Connectors,
    Settings,
}

/// The modules of the right column, top to bottom.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub enum Right {
    Arbiter,
    Context,
    Activity,
    Git,
    Files,
    Todo,
    Agents,
    Jobs,
}

/// The model menu over the composer, and which of its submenus is open.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Picker {
    Mode,
    Model,
    Effort,
    MoreModels,
}

pub struct Workspace {
    pub core: Core,
    /// Where a chat without a project works: the workspace root.
    pub home: PathBuf,
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
    pub shelf: Shelf,
    pub shelf_search: Entity<InputState>,
    /// The right column's modules that are folded.
    pub folded: HashSet<Right>,
    pub modal: Option<Modal>,
    pub settings_page: SettingsPage,
    pub font_search: Entity<InputState>,
    /// The machine's font families, read once.
    pub fonts: Vec<String>,
    pub model_input: Entity<InputState>,
    pub outputs: Vec<OutputFile>,
    pub artifact_kind: Option<OutputKind>,
    pub search: Entity<InputState>,
    pub composer: Entity<TextareaState>,
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
    pub picker_closed: Option<std::time::Instant>,
    /// Options marked in the shown chat's question.
    pub picked: Vec<usize>,
    pub attach_open: bool,
    pub left_hidden: bool,
    pub right_hidden: bool,
    /// Files to send with the next message.
    pub attachments: Vec<PathBuf>,
    /// Messages written while the agent works: each sits above the composer,
    /// editable, and goes in queue order when the turn ends.
    pub pending: Vec<chat::queue::Pending>,
    /// Output tokens per second by thread, from the core's rate events.
    pub rates: HashMap<String, u64>,
    /// The shown chat's sent messages, by transcript row, for the navigator.
    pub sent_rows: Vec<(usize, String)>,
    pub navigator_open: bool,
    /// Fields for the settings a plugin lacks, in its detail window.
    pub secret_inputs: Vec<(String, Entity<InputState>)>,
    /// The agents graph's level: the names from the root down.
    pub agent_path: Vec<String>,
    /// The chat a right click opened the menu of, and where.
    pub chat_menu: Option<(String, Point<Pixels>)>,
    pub system: Option<dusk_protocol::system::SystemReport>,
    pub update: Option<dusk_protocol::update::UpdateInfo>,
    /// Answers whose reasoning is unfolded, by chat and item index.
    pub unfolded: HashSet<(String, usize)>,
    /// The shown chat's transcript as a virtual list, and the rows it holds.
    pub list: ListState,
    pub rows: Vec<crate::chat::messages::Row>,
    /// The chat the list was built for; another one starts it anew.
    pub list_chat: Option<String>,
    /// A row changed height, as when folded: measure every row again.
    pub measure_again: bool,
}

impl Workspace {
    pub fn new(
        core: Core,
        home: PathBuf,
        events: mpsc::UnboundedReceiver<Event>,
        status: watch::Receiver<Status>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search chats"));
        cx.subscribe_in(&search, window, |_, _, _: &InputEvent, _, cx| cx.notify())
            .detach();
        let shelf_search = cx.new(|cx| InputState::new(window, cx).placeholder("Search"));
        cx.subscribe_in(&shelf_search, window, |_, _, _: &InputEvent, _, cx| {
            cx.notify()
        })
        .detach();
        let font_search = cx.new(|cx| InputState::new(window, cx).placeholder("Find a font"));
        cx.subscribe_in(&font_search, window, |_, _, _: &InputEvent, _, cx| {
            cx.notify()
        })
        .detach();
        let model_input = cx.new(|cx| InputState::new(window, cx).placeholder("provider/model"));
        cx.subscribe_in(&model_input, window, |ws, _, event: &InputEvent, _, cx| {
            if let InputEvent::PressEnter { .. } = event {
                ws.use_typed_model(cx);
            }
        })
        .detach();
        let composer = cx.new(|cx| {
            let mut state = TextareaState::new(window, cx)
                .auto_grow(2, 10)
                .placeholder("Ask anything. Enter sends, Shift+Enter starts a new line.");
            state.set_submit_on_enter(true, cx);
            state
        });
        cx.subscribe_in(
            &composer,
            window,
            |this, _, event: &InputEvent, window, cx| {
                if let InputEvent::PressEnter { shift: false, .. } = event {
                    this.send(window, cx);
                }
            },
        )
        .detach();
        let first = status.borrow().clone();
        let mut workspace = Self {
            core,
            home,
            status: first,
            projects: Vec::new(),
            threads: Vec::new(),
            chats: HashMap::new(),
            shown: None,
            project: None,
            collapsed: HashSet::new(),
            center: Center::Chat,
            shelf: Shelf::Connectors,
            shelf_search,
            folded: [Right::Files, Right::Jobs].into_iter().collect(),
            modal: None,
            settings_page: SettingsPage::Appearance,
            font_search,
            fonts: crate::theme::font_families(cx),
            model_input,
            outputs: Vec::new(),
            artifact_kind: None,
            search,
            composer,
            notice: None,
            pending: Vec::new(),
            rates: HashMap::new(),
            git: None,
            files: BTreeMap::new(),
            expanded: HashSet::new(),
            context: None,
            jobs: Vec::new(),
            settings: None,
            providers: Vec::new(),
            picker: None,
            picker_closed: None,
            picked: Vec::new(),
            attach_open: false,
            left_hidden: false,
            right_hidden: false,
            attachments: Vec::new(),
            sent_rows: Vec::new(),
            navigator_open: false,
            secret_inputs: Vec::new(),
            agent_path: Vec::new(),
            chat_menu: None,
            system: None,
            update: None,
            unfolded: HashSet::new(),
            list: ListState::new(0, ListAlignment::Top, px(800.)),
            rows: Vec::new(),
            list_chat: None,
            measure_again: false,
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
                if let dusk_protocol::thread::ThreadEvent::OutputRate { tokens_per_sec, .. } =
                    &*event
                {
                    self.rates.insert(thread_id.clone(), *tokens_per_sec);
                }
                let titled = matches!(
                    *event,
                    dusk_protocol::thread::ThreadEvent::TitleChanged { .. }
                );
                let mut finished = false;
                if let Some(chat) = self.chats.get_mut(&thread_id) {
                    let was_working = chat.working;
                    chat.apply(*event);
                    finished = was_working && !chat.working;
                }
                if finished {
                    self.rates.remove(&thread_id);
                    self.send_next_pending(&thread_id, cx);
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
            Center::Artifacts => artifacts::render(self, window, cx).into_any_element(),
            Center::Connectors => connectors::render(self, window, cx).into_any_element(),
            Center::Settings => settings::render(self, window, cx).into_any_element(),
        };
        let modal = modal::render(self, window, cx);
        let chat_menu = modal::chat_menu(self, cx);
        div()
            .relative()
            .size_full()
            .flex()
            .flex_row()
            .bg(background)
            .text_color(foreground)
            .map(|row| {
                if self.left_hidden {
                    row.child(sidebar::rail(self, cx))
                } else {
                    row.child(sidebar::render(self, window, cx))
                }
            })
            .child(div().flex_1().min_w_0().h_full().child(center))
            .when(!self.right_hidden, |row| {
                row.child(inspector::render(self, window, cx))
            })
            .children(modal)
            .children(chat_menu)
    }
}
