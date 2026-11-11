//! What the whole page knows: the connection, the status, the lists in the sidebar and the
//! chats that are open, as signals the views read. Everything here comes from the core.

use crate::{
    api,
    chat::data::ChatData,
    link::{self, Conn, Handlers},
    pages::{connectors::Tab, settings::Page},
    theme,
};
use dusk_protocol::{
    client::{Event, Status},
    project::Project,
    thread::{Thread, ThreadEvent},
};
use leptos::{prelude::*, task::spawn_local};
use std::collections::HashMap;

pub type Chat = RwSignal<ChatData>;

#[derive(Clone, PartialEq)]
pub enum View {
    /// A chat; none is a draft whose thread is made with its first message.
    Chat(Option<String>),
    Settings(Page),
    Connectors(Tab),
    Artifacts,
    Project(String),
}

#[derive(Clone, Copy)]
pub struct AppState {
    pub conn: RwSignal<Conn>,
    pub status: RwSignal<Option<Status>>,
    pub threads: RwSignal<Vec<Thread>>,
    pub projects: RwSignal<Vec<Project>>,
    pub chats: RwSignal<HashMap<String, Chat>>,
    pub view: RwSignal<View>,
    /// The project a draft chat starts in and the sidebar highlights.
    pub project: RwSignal<Option<String>>,
    pub toast: RwSignal<Option<String>>,
    pub theme: RwSignal<String>,
    /// A subagent chat shown in a window over the page: its parent chat and thread.
    pub subagent: RwSignal<Option<(String, String)>>,
    /// Whether an answer is being read aloud.
    pub speaking: RwSignal<bool>,
}

thread_local! {
    static APP: std::cell::Cell<Option<AppState>> = const { std::cell::Cell::new(None) };
}

/// The page's state. Kept apart from the component tree, so a handler can read it
/// wherever it runs.
pub fn state() -> AppState {
    APP.with(|app| app.get()).expect("the shell installs the state first")
}

pub fn install(app: AppState) {
    APP.with(|slot| slot.set(Some(app)));
}

impl AppState {
    pub fn new() -> Self {
        Self {
            conn: RwSignal::new(Conn::Connecting),
            status: RwSignal::new(None),
            threads: RwSignal::new(Vec::new()),
            projects: RwSignal::new(Vec::new()),
            chats: RwSignal::new(HashMap::new()),
            view: RwSignal::new(View::Chat(None)),
            project: RwSignal::new(None),
            toast: RwSignal::new(None),
            theme: RwSignal::new(theme::stored()),
            subagent: RwSignal::new(None),
            speaking: RwSignal::new(false),
        }
    }

    /// Opens the stream to the core and keeps the lists fresh.
    pub fn connect(self) {
        link::connect(Handlers {
            on_event: Box::new(move |event| self.on_event(event)),
            on_status: Box::new(move |status| self.status.set(Some(status))),
            on_conn: Box::new(move |conn| {
                self.conn.set(conn);
                if conn == Conn::Online {
                    self.after_connect();
                }
            }),
        });
    }

    fn after_connect(self) {
        self.refresh();
        // A stream that broke took its open threads with it; join them again.
        let open: Vec<String> = self.chats.with_untracked(|chats| chats.keys().cloned().collect());
        for thread_id in open {
            spawn_local(async move {
                let cwd = self.cwd_of(&thread_id);
                if api::resume(&thread_id, cwd).await.is_ok()
                    && let Ok(history) = api::messages(&thread_id).await
                    && let Some(chat) = self.chat(&thread_id)
                {
                    chat.update(|data| {
                        let (tasks, access) = (data.tasks.clone(), data.access);
                        *data = ChatData::from_history(&history);
                        (data.tasks, data.access) = (tasks, access);
                    });
                }
            });
        }
    }

    pub fn fail(self, message: impl Into<String>) {
        self.toast.set(Some(message.into()));
    }

    pub fn chat(self, thread_id: &str) -> Option<Chat> {
        self.chats.with_untracked(|chats| chats.get(thread_id).copied())
    }

    /// Reloads the sidebar lists.
    pub fn refresh(self) {
        spawn_local(async move {
            match api::threads().await {
                Ok(threads) => self.threads.set(threads),
                Err(error) => self.fail(error),
            }
            match api::projects().await {
                Ok(projects) => self.projects.set(projects),
                Err(error) => self.fail(error),
            }
        });
    }

    fn on_event(self, event: Event) {
        match event {
            Event::Thread { thread_id, event } => {
                let listed = self
                    .threads
                    .with_untracked(|threads| threads.iter().any(|t| t.id == thread_id));
                let renamed = matches!(*event, ThreadEvent::TitleChanged { .. });
                match self.chat(&thread_id) {
                    Some(chat) => chat.update(|data| data.apply(*event)),
                    None if !listed => self.refresh(),
                    None => {}
                }
                if renamed {
                    self.refresh();
                }
            }
            Event::Ended { thread_id } => {
                if let Some(chat) = self.chat(&thread_id) {
                    chat.update(|data| {
                        data.ended = true;
                        data.working = false;
                    });
                }
            }
        }
    }

    /// The folder a thread works in: its project's, or the workspace root.
    fn cwd_of(self, thread_id: &str) -> String {
        let project = self.threads.with_untracked(|threads| {
            threads
                .iter()
                .find(|t| t.id == thread_id)
                .and_then(|t| t.project.clone())
        });
        project.unwrap_or_else(|| self.workspace_dir())
    }

    pub fn workspace_dir(self) -> String {
        self.status.with_untracked(|status| {
            status
                .as_ref()
                .map(|s| s.workspace_dir.display().to_string())
                .unwrap_or_default()
        })
    }

    pub fn project_by_id(self, id: &str) -> Option<Project> {
        self.projects
            .with_untracked(|projects| projects.iter().find(|p| p.id == id).cloned())
    }

    /// A draft chat in a project, or in the workspace root without one.
    pub fn new_chat(self, project: Option<String>) {
        self.project.set(project);
        self.view.set(View::Chat(None));
    }

    /// Opens a listed chat: its history, then the thread itself.
    pub fn open_chat(self, thread_id: String) {
        if self.chat(&thread_id).is_some() {
            self.view.set(View::Chat(Some(thread_id)));
            return;
        }
        spawn_local(async move {
            let cwd = self.cwd_of(&thread_id);
            let history = api::messages(&thread_id).await;
            let opened = api::resume(&thread_id, cwd).await;
            let (history, thread_id) = match (history, opened) {
                (Ok(history), Ok(id)) => (history, id),
                (Err(error), _) | (_, Err(error)) => return self.fail(error),
            };
            let mut data = ChatData::from_history(&history);
            data.tasks = api::tasks(&thread_id).await.unwrap_or_default();
            data.access = api::get_approvals(&thread_id).await.unwrap_or_default();
            self.chats.update(|chats| {
                chats.insert(thread_id.clone(), RwSignal::new(data));
            });
            self.view.set(View::Chat(Some(thread_id)));
        });
    }

    /// Sends a message. From a draft the thread is made first, with this message.
    pub fn send(self, thread_id: Option<String>, text: String, now: bool) {
        spawn_local(async move {
            let thread_id = match thread_id {
                Some(id) => id,
                None => match self.start_thread().await {
                    Ok(id) => id,
                    Err(error) => return self.fail(error),
                },
            };
            if let Some(chat) = self.chat(&thread_id) {
                chat.update(|data| data.sent(text.clone()));
                if chat.with_untracked(|data| data.ended) {
                    let cwd = self.cwd_of(&thread_id);
                    let _ = api::resume(&thread_id, cwd).await;
                    chat.update(|data| data.ended = false);
                }
            }
            if let Err(error) = api::send(&thread_id, text, now).await {
                self.fail(error);
            }
        });
    }

    async fn start_thread(self) -> Result<String, String> {
        let cwd = match self.project.get_untracked() {
            Some(id) => self
                .project_by_id(&id)
                .map(|p| p.dir.display().to_string())
                .unwrap_or_else(|| self.workspace_dir()),
            None => self.workspace_dir(),
        };
        let thread_id = api::new_thread(cwd).await?;
        self.chats.update(|chats| {
            chats.insert(thread_id.clone(), RwSignal::new(ChatData::default()));
        });
        self.view.set(View::Chat(Some(thread_id.clone())));
        self.refresh();
        Ok(thread_id)
    }

    /// Stops using a chat on this page; the core closes it when no client keeps it.
    pub fn close_chat(self, thread_id: String) {
        spawn_local(async move {
            let _ = api::close(&thread_id).await;
            self.chats.update(|chats| {
                chats.remove(&thread_id);
            });
            if self.view.get_untracked() == View::Chat(Some(thread_id)) {
                self.view.set(View::Chat(None));
            }
        });
    }
}
