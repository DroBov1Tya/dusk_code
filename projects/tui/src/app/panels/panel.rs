//! The panel shown over the transcript, and the small views some panels hold.

use ratatui::layout::Rect;

use dusk_core::{
    chat::commands::ContextReport,
    config::ProviderKind,
    release::update::{InstallProgress, UpdateInfo},
    runtime::model_choice::ProviderStatus,
    state::{Message, MessageRole, Source, Thread, UndoPoint},
    tools::JobInfo,
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
    /// The Telegram bots of this session.
    Bots {
        selected: usize,
    },
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
    Jobs(JobsView),
    /// What is attached to the thread; `removing` asks to press the key again.
    Sources {
        sources: Vec<Source>,
        selected: usize,
        removing: bool,
    },
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
    /// Among `choices`.
    pub selected: usize,
}

impl ModelList {
    pub fn checking(current: Option<String>) -> Self {
        Self {
            current,
            providers: None,
            selected: 0,
        }
    }

    pub fn checked(current: Option<String>, providers: Vec<ProviderStatus>) -> Self {
        let mut list = Self {
            current,
            providers: Some(providers),
            selected: 0,
        };
        list.selected = list
            .choices()
            .iter()
            .position(|name| Some(name) == list.current.as_ref())
            .unwrap_or(0);
        list
    }

    /// Every model of every provider that answered, as `provider/model`, in order.
    pub fn choices(&self) -> Vec<String> {
        let providers = self.providers.iter().flatten();
        providers
            .flat_map(|provider| {
                let models = provider.models.as_deref().unwrap_or_default();
                models
                    .iter()
                    .map(|model| format!("{}/{}", provider.name, model.id))
            })
            .collect()
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

/// A Telegram bot as the bots panel shows it.
pub struct BotRow {
    pub id: usize,
    pub username: String,
    /// Who paired with it; `None` while it waits for its code.
    pub owner: Option<String>,
    pub code: String,
    pub running: bool,
}

/// Where /connect starts: the providers to pick from, each with what it needs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Preset {
    OpenAi,
    Anthropic,
    Ollama,
    Custom,
}

impl Preset {
    pub const ALL: [Self; 4] = [Self::OpenAi, Self::Anthropic, Self::Ollama, Self::Custom];

    pub fn label(self) -> &'static str {
        match self {
            Self::OpenAi => "OpenAI",
            Self::Anthropic => "Anthropic",
            Self::Ollama => "Ollama, on this machine",
            Self::Custom => "Custom, OpenAI-compatible",
        }
    }

    pub fn about(self) -> &'static str {
        match self {
            Self::OpenAi => "GPT models with an API key from the OpenAI platform.",
            Self::Anthropic => {
                "Claude models with an API key from the Anthropic console. A Claude \
                 subscription works only in Anthropic's own apps."
            }
            Self::Ollama => {
                "Models Ollama serves locally, no key. For long tasks start Ollama with a \
                 larger OLLAMA_CONTEXT_LENGTH; its default window is small."
            }
            Self::Custom => "Any server with the OpenAI API: an HTTPS URL, a key if it needs one.",
        }
    }

    pub fn kind(self) -> ProviderKind {
        match self {
            Self::OpenAi => ProviderKind::OpenAi,
            Self::Anthropic => ProviderKind::Anthropic,
            Self::Ollama | Self::Custom => ProviderKind::OpenAiCompatible,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::OpenAi => "openai",
            Self::Anthropic => "anthropic",
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
            Self::Ollama | Self::Custom => None,
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
