//! The window's one root view: what it knows of the core, which tab each column shows,
//! and the three columns themselves. Each column's module adds its own methods here.
//! - `new`: the view's first state and the streams it follows;
//! - `events`: what the core sends, folded into that state;
//! - `render`: the window drawn from it;
//! - `keys`: the shortcuts and what Escape closes;
//! - `seen`: telling the core a chat was read while it is shown;
//! - `toast`: short notes over any page that go away on their own.

mod events;
pub mod keys;
mod new;
mod render;
mod seen;
mod toast;

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::PathBuf,
};

use dusk_protocol::{
    client::Status,
    config::Settings,
    model::ProviderStatus,
    project::{FileEntry, GitInfo, OutputFile, OutputKind, Project},
    thread::{ContextReport, JobInfo, Thread},
};
use gpui_kit::{
    component::input::{InputState, TextareaState},
    *,
};

use crate::{
    columns::topbar,
    core::Core,
    modal::Modal,
    pages::{chat, chat::transcript::Transcript, customize::Shelf, settings::SettingsPage},
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Center {
    Chat,
    Artifacts,
    Customize,
    Settings,
    /// The project's changes, file by file.
    Review,
    /// One file, read-only, as an editor shows it.
    File,
    /// Prompts the core runs on a timetable.
    Schedules,
    /// How the shown chat's work stands, as a page.
    Overview,
    /// The team that runs the shown chat's plan, as a graph.
    Team,
}

/// The modules of the right column, in their default order top to bottom.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Right {
    Arbiter,
    /// How the shown chat's work stands: plan, team, tokens, context, changes, activity
    /// and its log.
    Overview,
    /// The team that runs the shown chat's plan, in rows.
    Team,
    Screenshots,
    Git,
    Files,
    /// The files the shown chat's commands changed.
    Touched,
    Plan,
    Todo,
    Jobs,
    /// The files and pastes attached to the shown chat.
    Sources,
}

impl Right {
    pub const ALL: [Right; 11] = [
        Right::Arbiter,
        Right::Overview,
        Right::Team,
        Right::Screenshots,
        Right::Git,
        Right::Files,
        Right::Touched,
        Right::Plan,
        Right::Todo,
        Right::Jobs,
        Right::Sources,
    ];

    /// The name the choices of folding and order are kept under.
    pub fn key(self) -> &'static str {
        match self {
            Self::Arbiter => "arbiter",
            Self::Overview => "overview",
            Self::Team => "team",
            Self::Screenshots => "screenshots",
            Self::Git => "git",
            Self::Files => "files",
            Self::Touched => "touched",
            Self::Plan => "plan",
            Self::Todo => "todo",
            Self::Jobs => "jobs",
            Self::Sources => "sources",
        }
    }

    /// What the work itself is about stays open; the rest opens when asked.
    fn folded_at_first() -> HashSet<Right> {
        [
            Right::Arbiter,
            Right::Overview,
            Right::Screenshots,
            Right::Git,
            Right::Files,
            Right::Touched,
            Right::Jobs,
            Right::Sources,
        ]
        .into_iter()
        .collect()
    }

    /// The modules folded as the user left them, or at first start.
    pub fn folded_from(kept: Option<&[String]>) -> HashSet<Right> {
        match kept {
            Some(keys) => Self::ALL
                .into_iter()
                .filter(|module| keys.iter().any(|key| key == module.key()))
                .collect(),
            None => Self::folded_at_first(),
        }
    }
}

/// The model menu over the composer, and which of its submenus is open.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Picker {
    Mode,
    Access,
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
    /// The timetable, once read; `None` until the page asks.
    pub schedules: Option<Vec<dusk_protocol::project::Schedule>>,
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
    /// Where the Customize page is scrolled to; a shelf shown whole starts at its top.
    pub shelf_scroll: ScrollHandle,
    /// The right column's light modules that are folded; a heavy one opens as a screen.
    pub folded: HashSet<Right>,
    /// The heavy module open as the right column's screen, and how many times a screen
    /// opened or closed, so each move slides once.
    pub right_screen: Option<Right>,
    pub right_moves: usize,
    /// Where the right column's list is scrolled, kept while a screen covers it.
    pub right_scroll: ScrollHandle,
    /// The right column's modules top to bottom, every one once.
    pub right_order: Vec<Right>,
    /// While a module's header is dragged: the module under the pointer, and whether the
    /// drop lands below it rather than above.
    pub module_drop: Option<(Right, bool)>,
    /// The module moved last, the side it came from in pixels (negative: from above),
    /// and when, so it settles into its place once.
    pub module_moved: Option<(Right, f32, std::time::Instant)>,
    /// The todo module shows its finished tasks; they are folded away by default.
    pub todo_done_open: bool,
    pub modal: Option<Modal>,
    pub settings_page: SettingsPage,
    pub ai_tab: crate::pages::settings::ai::Tab,
    pub font_search: Entity<InputState>,
    /// The machine's font families, read once.
    pub fonts: Vec<String>,
    pub model_input: Entity<InputState>,
    /// A new branch prefix, typed in the version control settings.
    pub branch_input: Entity<InputState>,
    /// The port the web server listens on, typed beside its switch.
    pub web_port: Entity<InputState>,
    pub outputs: Vec<OutputFile>,
    pub artifact_kind: Option<OutputKind>,
    pub search: Entity<InputState>,
    /// What the search found in the text of the chats, and the words it was for.
    pub found: (String, Vec<dusk_protocol::thread::Found>),
    /// A chat opened from a found message scrolls to these words once it shows.
    pub jump: Option<(String, String)>,
    /// The chat about to start is a temporary one: kept in memory, gone with the app.
    pub temporary_next: bool,
    pub composer: Entity<TextareaState>,
    /// Text for the composer, put there on the next frame, which has the window.
    pub fill_composer: Vec<chat::composer::Edit>,
    /// What is typed and attached in the chats that are not shown.
    pub drafts: chat::composer::Drafts,
    /// The profile and snippets a message names with `@`, and their settings fields.
    pub me: crate::pages::settings::about_me::AboutMe,
    pub arbiter: crate::pages::settings::arbiter::ArbiterForm,
    /// The last error, shown until it is dismissed.
    pub notice: Option<String>,
    /// A short note over any page, and when it came; see `toast`.
    pub toast: Option<(std::time::Instant, String)>,
    pub git: Option<GitInfo>,
    /// The shown project's work tree and pull requests, kept fresh by the core.
    pub repository: Option<dusk_protocol::project::RepositoryStatus>,
    /// Which groups of a pull request's checks are unfolded, by its number and the
    /// group's name; a group not here has its default (Failed open, the rest closed).
    pub checks_open: HashMap<(u64, &'static str), bool>,
    pub touched: Vec<dusk_protocol::project::Change>,
    pub review: crate::pages::review::Review,
    /// The window's own focus: shortcuts reach the workspace from here when no box or row
    /// has the keyboard.
    pub focus: FocusHandle,
    pub agents_trigger_focus: FocusHandle,
    /// The keys of a window that takes them: the subagents' list, a listing's gallery.
    pub modal_focus: FocusHandle,
    /// The rows of a subagent's chat window, drawn as a list like the chat's.
    pub agent_list: crate::modal::AgentList,
    /// The Workshop confirmation's Cancel button, where the keyboard starts.
    pub confirm_focus: FocusHandle,
    pub file: crate::pages::file::FileView,
    /// MCP servers whose restart button was just pressed.
    pub mcp_restarting: HashSet<String>,
    /// Folder entries by folder, `""` for the project's top.
    pub files: BTreeMap<String, Vec<FileEntry>>,
    pub expanded: HashSet<String>,
    pub context: Option<ContextReport>,
    pub context_load: crate::columns::inspector::context::ContextLoad,
    /// What the Team page keeps of its own.
    pub team_page: crate::pages::team::TeamPage,
    /// The core's answers the Overview draws.
    pub overview: crate::columns::inspector::overview::Shown,
    /// The plan strip and the views tab over the composer.
    pub dock: chat::dock::Dock,
    pub alerts: crate::platform::notify::Alerts,
    /// The shown chat's pinned plan.
    pub plan: crate::columns::inspector::plan::Plan,
    /// The boxes that connect a provider, on the Model page.
    pub providers_page: crate::pages::settings::providers::State,
    /// Which secrets are set, read when the Secrets page opens.
    pub secrets: Option<dusk_protocol::config::SecretsInfo>,
    /// The speech servers that are set up, and the boxes that edit them.
    pub voice: Option<dusk_protocol::config::VoiceSettings>,
    pub voice_models: Option<dusk_protocol::config::VoiceModels>,
    /// A poll of the models is waiting; there is one at a time.
    pub voice_polling: bool,
    /// The web server of this window's core, while the switch in Advanced is on.
    pub web: Option<dusk_server::WebServer>,
    /// The server is starting or stopping.
    pub web_busy: bool,
    /// The next start listens on every address, for other devices on the network.
    pub web_global: bool,
    pub voice_form: crate::pages::settings::ai::voice::VoiceForm,
    /// The plugin store on the plugins shelf.
    pub workshop: crate::pages::customize::plugins::Workshop,
    pub token_box: crate::pages::settings::TokenBox,
    /// Settings, Workshop: the repositories added and the Add form.
    pub repositories: crate::pages::settings::Repositories,
    /// The Advanced page's number being typed in place of its minus and plus.
    pub limit_edit: crate::pages::settings::LimitEdit,
    /// Parts of the context unfolded to what they are made of, by name.
    pub context_open: HashSet<String>,
    /// Providers unfolded in More models, by name; each starts folded.
    pub models_open: HashSet<String>,
    /// Pictures of the user's messages, by thread and source name, as the core gave them.
    pub sent_pictures: HashMap<(String, String), chat::messages::SentPicture>,
    /// The last click or key, for the mist that rises when the window is left alone.
    pub last_input: std::time::Instant,
    /// When the mist was cleared and how thick it stood, while it fades.
    pub fog_cleared: Option<(std::time::Instant, f32)>,
    /// When a turn ended under the mist and how thick it stood, while the dawn plays.
    pub fog_dawn: Option<(std::time::Instant, f32)>,
    /// The terminal panel at the foot of the work area.
    pub terminal: crate::columns::terminal::Panel,
    /// The parts plugins add, and the docks they are open in.
    pub plugins: crate::plugins::Plugins,
    pub jobs: Vec<JobInfo>,
    /// The shown chat's sources, as the core lists them.
    pub sources: Vec<dusk_protocol::thread::Source>,
    pub settings: Option<Settings>,
    pub providers: Vec<ProviderStatus>,
    pub models_loading: bool,
    /// Providers whose models have not come yet; each answers on its own.
    pub models_pending: HashSet<String>,
    /// Counts model loads, so answers from a load a newer one replaced are dropped.
    pub models_round: u64,
    pub picker: Option<Picker>,
    pub picker_closed: Option<std::time::Instant>,
    /// Options marked in the shown chat's question.
    pub picked: Vec<usize>,
    /// Approval command boxes shown in full, by request and step.
    pub approval_open: std::collections::HashSet<(u64, usize)>,
    pub attach_open: bool,
    pub left_hidden: bool,
    /// The strip of places at the left edge, hidden on the user's word.
    pub rail_hidden: bool,
    /// Quiet: only the chat and its box; the sidebar and the side panel step away.
    pub quiet: bool,
    /// A rail place shown on hover, and whether the pointer is on its icon or panel.
    pub flyout: Option<crate::columns::flyout::Flyout>,
    pub flyout_inside: bool,
    /// Side panel modules showing all their rows rather than the first few.
    pub unfolded_modules: HashSet<Right>,
    /// The places the user went, for Back and Forward.
    pub history: topbar::history::History,
    pub right_hidden: bool,
    /// Files to send with the next message.
    pub attachments: Vec<PathBuf>,
    /// Long pasted texts, sent as the core's paste tags so it keeps them as files.
    pub pastes: Vec<String>,
    /// Messages written while the agent works: each sits above the composer,
    /// editable, and goes in queue order when the turn ends.
    pub pending: Vec<chat::queue::Pending>,
    /// Output tokens per second by thread, from the core's rate events.
    pub rates: HashMap<String, u64>,
    /// The user's likely next message by chat, as the model proposed it.
    pub suggestions: HashMap<String, dusk_protocol::thread::Suggestion>,
    /// What the composer's empty box says now: the placeholder or a suggestion.
    pub placeholder_shown: String,
    /// When Tab last took a suggestion in, for the box's flare.
    pub suggestion_taken: Option<std::time::Instant>,
    /// The shown chat's sent messages, by transcript row, for the navigator.
    /// The rows of the user's sent messages: their row and their item.
    pub sent_rows: Vec<(usize, usize)>,
    /// What `rows` were built from: the chat, its shape and whether a turn ran.
    pub rows_key: Option<(String, u64, bool)>,
    pub navigator_open: bool,
    /// Fields for the settings a plugin lacks, in its detail window.
    pub secret_inputs: Vec<(String, Entity<InputState>)>,
    /// The agents graph's level: the names from the root down.
    pub agent_path: Vec<String>,
    /// The chat a right click opened the menu of, and where.
    pub chat_menu: Option<(String, Point<Pixels>)>,
    /// The artifact a right click opened the menu of, and where.
    pub artifact_menu: Option<(PathBuf, Point<Pixels>)>,
    /// Chats just deleted, fading out of the list before it is read again.
    pub fading: std::collections::HashSet<String>,
    /// The release the app fetched in the background and put in place; it runs after a
    /// restart.
    pub update_ready: Option<String>,
    pub audio: crate::pages::chat::audio::Audio,
    /// The microphone is on and its words go to the message box.
    pub dictation: Option<crate::pages::chat::dictation::Dictation>,
    /// An answer is being turned into sound, to be played.
    pub synthesizing: bool,
    /// The release a background fetch was started for, so one failing is not retried at
    /// every status change.
    pub update_tried: Option<String>,
    /// The menu of the project folder in the right column is open.
    pub path_menu: bool,
    pub system: Option<dusk_protocol::system::SystemReport>,
    pub update: Option<dusk_protocol::release::UpdateInfo>,
    /// Answers whose reasoning is unfolded, by chat and item index.
    pub unfolded: HashSet<(String, usize)>,
    /// The shown chat's transcript as a virtual list, and the rows it holds.
    pub list: ListState,
    pub rows: Vec<crate::pages::chat::messages::Row>,
    /// The chat the list was built for; another one starts it anew.
    pub list_chat: Option<String>,
    /// The parsed words of that chat's rows, so a row scrolled back comes at its height.
    pub texts: crate::pages::chat::messages::Texts,
    /// A row changed height, as when folded: measure every row again.
    pub measure_again: bool,
    /// How wide the transcript's rows are drawn, for blocks that set their columns by it.
    pub reading_width: f32,
    /// The chat whose reply buttons were just pressed and how many items it had then,
    /// so one press sends once.
    pub replied: Option<(String, usize)>,
    /// The node of a diagram the pointer is on, by chat, item index and node, which
    /// lights the edges that touch it.
    pub diagram_hover: Option<(String, usize, usize)>,
    /// The pinned bar: the pin it shows, whether its list is open, a pin under way.
    pub pins: chat::pins::PinsView,
    /// The first screen while no provider is connected.
    pub welcome: crate::pages::welcome::Welcome,
}

impl Workspace {
    /// Shows an error until it is dismissed; also kept in the log.
    pub fn fail(&mut self, error: anyhow::Error, cx: &mut Context<Self>) {
        tracing::warn!("{error:#}");
        self.notice = Some(format!("{error:#}"));
        cx.notify();
    }

    pub fn selected_project(&self) -> Option<&Project> {
        let id = self.project.as_deref()?;
        self.projects.iter().find(|project| project.id == id)
    }

    pub fn shown_chat(&self) -> Option<&Transcript> {
        self.chats.get(self.shown.as_deref()?)
    }
}
