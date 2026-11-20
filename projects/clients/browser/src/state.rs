//! What the whole page knows: the connection, the status, the lists in the sidebar and the
//! chats that are open, as signals the views read. Everything here comes from the core.

use crate::{
    api,
    chat::data::ChatData,
    link::{self, Conn, Handlers},
    pages::{customize::Tab, settings::Page},
    ui::theme,
};
use dusk_protocol::{
    client::{Event, Status},
    project::{Project, RepositoryStatus},
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
    Customize(Tab),
    Artifacts,
    Schedules,
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
    navigation: RwSignal<u64>,
    pub draft_generation: RwSignal<u64>,
    /// The project a draft chat starts in and the sidebar highlights.
    pub project: RwSignal<Option<String>>,
    /// The chat about to start is a temporary one: kept in memory, gone with the core.
    pub temporary_next: RwSignal<bool>,
    /// Unsent messages by chat, none for the draft chat; in memory only.
    drafts: StoredValue<HashMap<Option<String>, crate::chat::Draft>>,
    pub dictations: RwSignal<HashMap<Option<String>, Vec<String>>>,
    pub pending_pictures: RwSignal<HashMap<Option<String>, Vec<crate::chat::Picture>>>,
    pub toast: RwSignal<Option<String>>,
    pub theme: RwSignal<String>,
    /// A subagent chat shown in a window over the page: its parent chat and thread.
    pub subagent: RwSignal<Option<(String, String)>>,
    /// Whether an answer is being read aloud.
    pub speaking: RwSignal<bool>,
    /// The last repository status the core sent, and the project it is of.
    pub repository: RwSignal<Option<(String, RepositoryStatus)>>,
}

thread_local! {
    static APP: std::cell::Cell<Option<AppState>> = const { std::cell::Cell::new(None) };
}

/// The page's state. Kept apart from the component tree, so a handler can read it
/// wherever it runs.
pub fn state() -> AppState {
    APP.with(|app| app.get())
        .expect("the shell installs the state first")
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
            navigation: RwSignal::new(0),
            draft_generation: RwSignal::new(0),
            project: RwSignal::new(None),
            temporary_next: RwSignal::new(false),
            drafts: StoredValue::new(HashMap::new()),
            dictations: RwSignal::new(HashMap::new()),
            pending_pictures: RwSignal::new(HashMap::new()),
            toast: RwSignal::new(None),
            theme: RwSignal::new(theme::stored()),
            subagent: RwSignal::new(None),
            speaking: RwSignal::new(false),
            repository: RwSignal::new(None),
        }
    }

    /// Opens the stream to the core and keeps the lists fresh.
    /// `signed_out` runs when the stream keeps failing because the session is gone, as
    /// after the server restarted.
    pub fn connect(self, signed_out: impl Fn() + 'static) {
        let signed_out = std::rc::Rc::new(signed_out);
        link::connect(Handlers {
            on_event: Box::new(move |event| self.on_event(event)),
            on_status: Box::new(move |status| self.status.set(Some(status))),
            on_conn: Box::new(move |conn| {
                self.conn.set(conn);
                match conn {
                    Conn::Online => self.after_connect(),
                    Conn::Lost => {
                        let signed_out = std::rc::Rc::clone(&signed_out);
                        spawn_local(async move {
                            if !crate::auth::is_signed_in().await {
                                signed_out();
                            }
                        });
                    }
                    Conn::Connecting => {}
                }
            }),
        });
    }

    fn after_connect(self) {
        self.refresh();
        // A stream that broke took its open threads with it; join them again.
        let open: Vec<String> = self
            .chats
            .with_untracked(|chats| chats.keys().cloned().collect());
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

    /// Shows a message over the page for a few seconds.
    pub fn fail(self, message: impl Into<String>) {
        let message = message.into();
        self.toast.set(Some(message.clone()));
        spawn_local(async move {
            crate::ui::sleep(8000).await;
            // Only its own message goes away; a newer one stays its full time.
            self.toast.update(|now| {
                if now.as_deref() == Some(message.as_str()) {
                    *now = None;
                }
            });
        });
    }

    pub fn chat(self, thread_id: &str) -> Option<Chat> {
        self.chats
            .with_untracked(|chats| chats.get(thread_id).copied())
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
                // /fork wrote its summary: the new chat starts from it.
                if let ThreadEvent::Forked { summary } = &*event {
                    self.fork(thread_id.clone(), summary.clone());
                }
                match self.chat(&thread_id) {
                    // Streamed text is told to its item alone; the rest to the whole chat.
                    Some(chat) => {
                        let applied = chat.try_update_untracked(|data| data.apply(*event));
                        let told = applied.is_some_and(|applied| {
                            chat.with_untracked(|data| data.told(&applied))
                        });
                        if !told {
                            chat.notify();
                        }
                    }
                    None if !listed => self.refresh(),
                    None => {}
                }
                if renamed {
                    self.refresh();
                }
            }
            // A terminal is the desktop window's; the core never sends one to a browser.
            // Terminals and plugin parts are drawn by the desktop app only.
            Event::Terminal { .. }
            | Event::TerminalClosed { .. }
            | Event::PluginView { .. }
            | Event::PluginFrame { .. }
            | Event::PluginNotice { .. }
            // The page notices the closing socket itself.
            | Event::CoreStopping { .. }
            // The project page reads the repository when it opens.
            | Event::GitChanged { .. } => {}
            Event::RepositoryStatus { project_id, status } => {
                self.repository.set(Some((project_id, *status)))
            }
            Event::Ended { thread_id } => {
                if let Some(chat) = self.chat(&thread_id) {
                    chat.update(|data| {
                        data.ended = true;
                        data.working = false;
                        data.live = Default::default();
                    });
                }
            }
        }
    }

    /// Selecting a page supersedes pending navigation requests.
    pub fn show(self, view: View) {
        self.navigation.update(|n| *n = n.wrapping_add(1));
        self.view.set(view);
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

    fn advance_draft(self) {
        self.draft_generation.update(|n| *n = n.wrapping_add(1));
        self.dictations.update(|pending| {
            pending.remove(&None);
        });
        if let Some(pictures) = self
            .pending_pictures
            .try_update(|pending| pending.remove(&None).unwrap_or_default())
        {
            for picture in pictures {
                picture.forget();
            }
        }
    }

    /// A draft chat in a project, or in the workspace root without one.
    pub fn new_chat(self, project: Option<String>) {
        self.project.set(project);
        self.advance_draft();
        self.temporary_next.set(false);
        self.show(View::Chat(None));
    }

    /// A new chat that is never written to disk, made with its first message.
    pub fn new_temporary_chat(self) {
        self.new_chat(self.project.get_untracked());
        self.temporary_next.set(true);
        self.toast
            .set(Some("Temporary chat: kept in memory only".to_owned()));
    }

    /// The unsent message of a chat, handed to the box that shows it.
    pub fn take_draft(self, chat: &Option<String>) -> crate::chat::Draft {
        self.drafts
            .try_update_value(|drafts| drafts.remove(chat))
            .flatten()
            .unwrap_or_default()
    }

    /// Keeps what a chat's box held when it left the page. A chat that is no longer open
    /// here was closed or deleted, and its draft goes with it.
    pub fn keep_draft(self, chat: Option<String>, draft: crate::chat::Draft) {
        let open = chat.as_deref().is_none_or(|id| self.chat(id).is_some());
        if !open || (draft.text.is_empty() && draft.pictures.is_empty()) {
            return draft.pictures.iter().for_each(|picture| picture.forget());
        }
        self.drafts.update_value(|drafts| {
            drafts.insert(chat, draft);
        });
    }

    /// A closed or deleted chat takes its draft with it.
    pub fn drop_draft(self, thread_id: &str) {
        self.dictations.update(|pending| {
            pending.remove(&Some(thread_id.to_owned()));
        });
        if let Some(pictures) = self.pending_pictures.try_update(|pending| {
            pending
                .remove(&Some(thread_id.to_owned()))
                .unwrap_or_default()
        }) {
            for picture in pictures {
                picture.forget();
            }
        }
        for picture in self.take_draft(&Some(thread_id.to_owned())).pictures {
            picture.forget();
        }
    }

    /// Opens a listed chat: its history first, then the thread itself.
    pub fn open_chat(self, thread_id: String) {
        self.navigation.update(|n| *n = n.wrapping_add(1));
        let navigation = self.navigation.get_untracked();
        if self.chat(&thread_id).is_some() {
            self.show(View::Chat(Some(thread_id)));
            return;
        }
        spawn_local(async move {
            let cwd = self.cwd_of(&thread_id);
            let history = match api::messages(&thread_id).await {
                Ok(history) => history,
                Err(error) => return self.fail(error),
            };
            // The chat is in place before the thread opens, so the answer to that and
            // the turn another client already runs are applied, not dropped.
            self.chats.update(|chats| {
                chats
                    .entry(thread_id.clone())
                    .or_insert_with(|| RwSignal::new(ChatData::from_history(&history)));
            });
            if let Err(error) = api::resume(&thread_id, cwd).await {
                self.chats.update(|chats| {
                    chats.remove(&thread_id);
                });
                return self.fail(error);
            }
            if self.navigation.get_untracked() == navigation {
                self.show(View::Chat(Some(thread_id.clone())));
            }
            let tasks = api::tasks(&thread_id).await.unwrap_or_default();
            let access = api::get_approvals(&thread_id).await.unwrap_or_default();
            if let Some(chat) = self.chat(&thread_id) {
                chat.update(|data| {
                    data.tasks = tasks;
                    data.access = access;
                });
            }
        });
    }

    /// Sends a message with its pictures. From a draft the thread is made first, with
    /// this message; the pictures go to the thread before the text that names them.
    pub fn send(
        self,
        thread_id: Option<String>,
        text: String,
        now: bool,
        pictures: Vec<crate::chat::Picture>,
    ) {
        let navigation = self.navigation.get_untracked();
        let cwd = match self.project.get_untracked() {
            Some(id) => self
                .project_by_id(&id)
                .map(|p| p.dir.display().to_string())
                .unwrap_or_else(|| self.workspace_dir()),
            None => self.workspace_dir(),
        };
        let temporary = self.temporary_next.get_untracked();
        if thread_id.is_none() {
            self.advance_draft();
            self.temporary_next.set(false);
        }
        spawn_local(async move {
            let thread_id = match thread_id {
                Some(id) => id,
                None => match self.start_thread(cwd, temporary, navigation).await {
                    Ok(id) => id,
                    Err(error) => return self.fail(error),
                },
            };
            let mut text = text;
            for picture in pictures {
                picture.forget();
                match api::upload_picture(&thread_id, picture.name, picture.bytes).await {
                    Ok(tag) => {
                        if !text.is_empty() {
                            text.push('\n');
                        }
                        text.push_str(&tag);
                    }
                    Err(error) => return self.fail(error),
                }
            }
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

    async fn start_thread(
        self,
        cwd: String,
        temporary: bool,
        navigation: u64,
    ) -> Result<String, String> {
        let thread_id = match temporary {
            true => api::new_temporary_thread(cwd).await?,
            false => api::new_thread(cwd).await?,
        };
        self.chats.update(|chats| {
            chats.insert(thread_id.clone(), RwSignal::new(ChatData::default()));
        });
        if self.navigation.get_untracked() == navigation {
            self.show(View::Chat(Some(thread_id.clone())));
        }
        self.refresh();
        Ok(thread_id)
    }

    fn fork(self, from: String, summary: String) {
        let navigation = self.navigation.get_untracked();
        spawn_local(async move {
            let cwd = self.cwd_of(&from);
            match api::fork(summary, cwd).await {
                Ok(thread_id) => {
                    self.chats.update(|chats| {
                        chats.insert(thread_id.clone(), RwSignal::new(ChatData::default()));
                    });
                    if self.navigation.get_untracked() == navigation {
                        self.show(View::Chat(Some(thread_id)));
                    }
                    self.refresh();
                }
                Err(error) => self.fail(error),
            }
        });
    }

    /// A new chat from `from`'s history up to and including `message_id`, shown once made.
    pub fn fork_at(self, from: String, message_id: String) {
        let navigation = self.navigation.get_untracked();
        spawn_local(async move {
            let cwd = self.cwd_of(&from);
            let made = async {
                let thread_id = api::fork_at(&from, &message_id, cwd).await?;
                let history = api::messages(&thread_id).await?;
                api::Res::Ok((thread_id, history))
            };
            match made.await {
                Ok((thread_id, history)) => {
                    let data = ChatData::from_history(&history);
                    self.chats.update(|chats| {
                        chats.insert(thread_id.clone(), RwSignal::new(data));
                    });
                    if self.navigation.get_untracked() == navigation {
                        self.show(View::Chat(Some(thread_id)));
                    }
                    self.refresh();
                }
                Err(error) => self.fail(error),
            }
        });
    }

    /// Stops using a chat on this page; the core closes it when no client keeps it.
    pub fn close_chat(self, thread_id: String) {
        spawn_local(async move {
            let _ = api::close(&thread_id).await;
            self.chats.update(|chats| {
                chats.remove(&thread_id);
            });
            self.drop_draft(&thread_id);
            if self.view.get_untracked() == View::Chat(Some(thread_id)) {
                self.show(View::Chat(None));
            }
        });
    }
}
