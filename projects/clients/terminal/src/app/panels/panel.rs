//! The panel shown over the transcript, and the small views some panels hold.

use ratatui::layout::Rect;

use dusk_protocol::{
    config::ProviderKind,
    model::ProviderStatus,
    release::{InstallProgress, UpdateInfo},
    thread::{ContextReport, JobInfo, Message, MessageRole, PlanFile, Source, Thread, UndoPoint},
};

use super::{AgentWindow, SettingsView};
use crate::app::{Input, PasteEditor};

pub enum Panel {
    Threads(ThreadList),
    Settings(Box<SettingsView>),
    Help,
    Paste(PasteEditor),
    Models(ModelList),
    Agent(AgentWindow),
    Connect(Connection),
    /// The plugins found, to switch on or off.
    Plugins {
        selected: usize,
        /// The masked form for a secret setting, while one is being typed.
        entry: Option<SecretEntry>,
    },
    /// The plugin store, opened from the plugins panel.
    Workshop(Box<super::Workshop>),
    Unsent {
        selected: usize,
    },
    /// The newest version and the button that installs it; `info` is `None` while the
    /// core asks GitHub, and `note` says what the last step did.
    Update {
        info: Option<UpdateInfo>,
        note: Option<String>,
        progress: Option<InstallProgress>,
        /// The update button on the last drawn frame, as (row, left, right).
        button: Option<(u16, u16, u16)>,
    },
    /// The reasoning efforts to choose from: default first.
    Effort {
        selected: usize,
    },
    /// The user's messages to take the thread back to, oldest first.
    Undo {
        points: Vec<UndoPoint>,
        selected: usize,
    },
    /// What the context holds; `row` is the line kept in view while scrolling.
    Context {
        report: ContextReport,
        row: usize,
    },
    /// How the thread's work stands, as text; read again while open.
    Overview(Box<OverviewView>),
    Jobs(JobsView),
    /// The chat's folder's changes: the files, or one of them open.
    Diff(DiffView),
    /// The timetable; `removing` asks to press the key again.
    Schedules {
        tasks: Vec<dusk_protocol::project::Schedule>,
        selected: usize,
        removing: bool,
    },
    /// What is attached to the thread; `removing` asks to press the key again.
    Sources {
        sources: Vec<Source>,
        selected: usize,
        removing: bool,
    },
    /// `/plans`: the chat's plans, newest written first; `asking` waits for `y`.
    Plans {
        plans: Vec<PlanFile>,
        selected: usize,
        asking: Option<PlansAsk>,
    },
}

/// What the plans panel asks `y` for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlansAsk {
    /// `d`: delete the chosen plan.
    Delete,
    /// `o`: delete every old plan.
    Old,
}

/// The core's Overview and context report for the /overview panel; the plan and team
/// are the app's, which the rail shows too.
pub struct OverviewView {
    pub overview: dusk_protocol::thread::Overview,
    pub context: ContextReport,
    /// The line kept in view while scrolling.
    pub row: usize,
    /// `s` asked "Stop team? y/n": `y` stops it, any other key answers no.
    pub stopping: bool,
    /// How many members of the running team work at once, being changed.
    pub at_once: Option<AtOnceEdit>,
    /// The member row chosen with tab, which enter opens.
    pub member: Option<usize>,
}

impl OverviewView {
    pub fn new(overview: dusk_protocol::thread::Overview, context: ContextReport) -> Self {
        Self {
            overview,
            context,
            row: 0,
            stopping: false,
            at_once: None,
            member: None,
        }
    }
}

/// How many members of a running team work at once, stepped with left and right.
pub struct AtOnceEdit {
    pub at_once: u32,
    /// Why the last save was refused.
    pub error: Option<String>,
}

pub struct DiffView {
    pub files: Vec<dusk_protocol::project::FileDiff>,
    pub selected: usize,
    /// One file is open, scrolled to `scroll`.
    pub open: bool,
    pub scroll: usize,
}

/// The thread's background jobs and the output of the chosen one, read again every second
/// while the window is open.
pub struct JobsView {
    pub jobs: Vec<JobInfo>,
    pub selected: usize,
    /// What the chosen job printed, as read at the last refresh.
    pub output: String,
    /// The choice or the jobs changed: read again at once.
    pub stale: bool,
}

/// The configured providers as the core found them, and their models to choose from.
pub struct ModelList {
    /// `provider/model` in use.
    pub current: Option<String>,
    /// `None` while the core checks the providers.
    pub providers: Option<Vec<ProviderStatus>>,
    /// Among `rows`.
    pub selected: usize,
    /// The provider a first `d` asked to remove; a second `d` removes it.
    pub removing: Option<String>,
}

/// A row of the model list: a provider, which can be checked again or removed, or one of
/// its models, which can be chosen.
#[derive(Clone, PartialEq)]
pub enum ModelRow {
    Provider(String),
    Model { provider: String, name: String },
}

impl ModelRow {
    pub fn provider(&self) -> &str {
        match self {
            ModelRow::Provider(provider) | ModelRow::Model { provider, .. } => provider,
        }
    }
}

impl ModelList {
    pub fn checking(current: Option<String>) -> Self {
        Self {
            current,
            providers: None,
            selected: 0,
            removing: None,
        }
    }

    pub fn checked(current: Option<String>, providers: Vec<ProviderStatus>) -> Self {
        let mut list = Self {
            current,
            providers: Some(providers),
            selected: 0,
            removing: None,
        };
        list.selected = list
            .rows()
            .iter()
            .position(|row| matches!(row, ModelRow::Model { name, .. } if Some(name) == list.current.as_ref()))
            .unwrap_or(0);
        list
    }

    /// Each provider followed by its models, as `provider/model`, in order.
    pub fn rows(&self) -> Vec<ModelRow> {
        let mut rows = Vec::new();
        for provider in self.providers.iter().flatten() {
            rows.push(ModelRow::Provider(provider.name.clone()));
            for model in provider.models.as_deref().unwrap_or_default() {
                rows.push(ModelRow::Model {
                    provider: provider.name.clone(),
                    name: format!("{}/{}", provider.name, model.id),
                });
            }
        }
        rows
    }
}

pub struct ThreadList {
    pub threads: Vec<Thread>,
    pub selected: usize,
    /// The selected thread at a glance, once loaded.
    pub preview: Option<Preview>,
}

/// What a thread is about without its whole history: how it began and how it stands.
pub struct Preview {
    pub thread_id: String,
    /// User messages and answers with text, oldest first.
    pub messages: Vec<(MessageRole, String)>,
    pub tool_calls: usize,
}

impl Preview {
    pub fn new(thread_id: String, history: &[Message]) -> Self {
        let messages = history
            .iter()
            .filter(|message| matches!(message.role, MessageRole::User | MessageRole::Assistant))
            .filter(|message| !message.content.trim().is_empty())
            .map(|message| (message.role, message.content.clone()))
            .collect();
        let tool_calls = history.iter().map(|message| message.tool_calls.len()).sum();
        Self {
            thread_id,
            messages,
            tool_calls,
        }
    }
}

/// A plugin as the plugins panel shows it.
pub struct PluginRow {
    pub name: String,
    pub version: String,
    pub duskcode_version: Option<String>,
    pub description: String,
    pub enabled: bool,
    /// What it brings, such as `mcp` or `platform`.
    pub parts: Vec<String>,
    /// Why it does not load or what its platform says, such as a code to pair with.
    pub note: Option<String>,
    /// Its secret settings and whether each is set for this session.
    pub secrets: Vec<(String, bool)>,
}

/// A plugin's secret being typed. It is shown as stars, kept in memory only and never
/// reaches the transcript, the history or a file.
pub struct SecretEntry {
    pub plugin: String,
    pub key: String,
    pub input: Input,
    pub error: Option<String>,
}

/// Where /connect starts: the providers to pick from, each with what it needs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Preset {
    /// No form: the browser signs in, and the plan's models come with it.
    ChatGpt,
    OpenAi,
    Anthropic,
    Google,
    Ollama,
    Custom,
}

impl Preset {
    pub const ALL: [Self; 6] = [
        Self::ChatGpt,
        Self::OpenAi,
        Self::Anthropic,
        Self::Google,
        Self::Ollama,
        Self::Custom,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::ChatGpt => "ChatGPT, sign in",
            Self::OpenAi => "OpenAI",
            Self::Anthropic => "Anthropic",
            Self::Google => "Google AI",
            Self::Ollama => "Ollama, on this machine",
            Self::Custom => "Custom, OpenAI-compatible",
        }
    }

    pub fn about(self) -> &'static str {
        match self {
            Self::ChatGpt => {
                "GPT models on your ChatGPT plan, no API key: Enter opens OpenAI's sign-in \
                 page in the browser."
            }
            Self::OpenAi => "GPT models with an API key from the OpenAI platform.",
            Self::Anthropic => {
                "Claude models with an API key from the Anthropic console. A Claude \
                 subscription works only in Anthropic's own apps."
            }
            Self::Google => "Gemini models with an API key from Google AI Studio.",
            Self::Ollama => {
                "Models Ollama serves locally, no key. For long tasks start Ollama with a \
                 larger OLLAMA_CONTEXT_LENGTH; its default window is small."
            }
            Self::Custom => "Any server with the OpenAI API: an HTTPS URL, a key if it needs one.",
        }
    }

    pub fn kind(self) -> ProviderKind {
        match self {
            Self::ChatGpt => ProviderKind::ChatGpt,
            Self::OpenAi => ProviderKind::OpenAi,
            Self::Anthropic => ProviderKind::Anthropic,
            Self::Google => ProviderKind::Google,
            Self::Ollama | Self::Custom => ProviderKind::OpenAiCompatible,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::ChatGpt => "chatgpt",
            Self::OpenAi => "openai",
            Self::Anthropic => "anthropic",
            Self::Google => "google",
            Self::Ollama => "ollama",
            Self::Custom => "",
        }
    }

    fn url(self) -> &'static str {
        match self {
            Self::Ollama => "http://127.0.0.1:11434/v1",
            _ => self.kind().base_url().unwrap_or_default(),
        }
    }

    /// Where a key is made, to open in the browser.
    pub fn key_page(self) -> Option<&'static str> {
        match self {
            Self::OpenAi => Some("https://platform.openai.com/api-keys"),
            Self::Anthropic => Some("https://console.anthropic.com/settings/keys"),
            Self::Google => Some("https://aistudio.google.com/apikey"),
            Self::ChatGpt | Self::Ollama | Self::Custom => None,
        }
    }

    pub fn needs_key(self) -> bool {
        self.key_page().is_some()
    }
}

#[derive(Default)]
pub struct Connection {
    /// None while the provider is picked from the list.
    pub preset: Option<Preset>,
    /// The provider under the cursor in the list.
    pub choice: usize,
    /// Name, base URL and API key.
    pub fields: [Input; 3],
    /// The field in focus.
    pub selected: usize,
    pub error: Option<String>,
    /// Visible list or field rows on the last drawn frame.
    pub field_areas: [Option<Rect>; Connection::ROWS],
}

impl Connection {
    /// The longer of the provider list and the form.
    pub const ROWS: usize = Preset::ALL.len();
    pub const FIELDS: usize = 3;

    pub fn field(&mut self) -> Option<&mut Input> {
        self.preset?;
        self.fields.get_mut(self.selected)
    }

    /// Opens the form for `preset`, its name and URL filled in, focus where typing
    /// starts.
    pub fn pick(&mut self, preset: Preset) {
        self.preset = Some(preset);
        self.fields = Default::default();
        self.fields[0].insert(preset.name());
        self.fields[1].insert(preset.url());
        self.selected = match preset {
            Preset::Custom => 1,
            _ => 2,
        };
        self.error = None;
    }

    pub fn back(&mut self) {
        self.preset = None;
        self.error = None;
    }
}

/// Moves a selection within `len` items, stopping at the ends.
pub fn step(selected: &mut usize, len: usize, down: bool) {
    if down {
        *selected = (*selected + 1).min(len.saturating_sub(1));
    } else {
        *selected = selected.saturating_sub(1);
    }
}
