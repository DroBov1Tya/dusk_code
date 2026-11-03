//! What the app asks the session to do, and the commands the box offers.

use dusk_core::{
    chat::commands::{COMMANDS, Command},
    config::{ContextPolicy, ProviderKind},
    tools::Decision,
};

use super::SwitchKind;

/// What the session does for the app.
pub enum Action {
    None,
    Quit,
    Send(String),
    SendNow(String),
    Cancel,
    /// The answer to a tool's request for approval.
    Approve {
        id: u64,
        decision: Decision,
    },
    /// Checks a Telegram bot token and starts the bot; the token stays in memory.
    StartBot(String),
    /// Stops a running bot or starts a stopped one again, by its id.
    ToggleBot(usize),
    RemoveBot(usize),
    Connect {
        kind: ProviderKind,
        /// `None` takes the next free `provider-N`.
        name: Option<String>,
        base_url: String,
        api_key: Option<String>,
    },
    SetAgentLimit(usize),
    SetGoalRounds(u32),
    SetIdleMinutes(u32),
    SetMemoryChars(usize),
    SetWorkHistoryChars(usize),
    PinSource {
        id: String,
        pinned: bool,
    },
    RemoveSource(String),
    /// Ask a running background job of the open thread to stop.
    StopJob(String),
    SetContextPolicy(ContextPolicy),
    /// Turn an agent, a tool, a skill or an MCP server on or off.
    Switch {
        kind: SwitchKind,
        name: String,
        on: bool,
    },
    /// Read a skill from disk into the settings, dropping unsaved edits.
    OpenSkill(String),
    SaveSkill {
        name: String,
        text: String,
    },
    /// Read an agent's AGENT.md into the settings, dropping unsaved edits.
    OpenAgentFile(String),
    SaveAgentFile {
        name: String,
        text: String,
    },
    /// Read an MCP server's YAML into the settings; `None` starts a new one.
    OpenMcp(Option<String>),
    /// Save an MCP server in place of `old`; empty text removes it.
    SaveMcp {
        old: Option<String>,
        text: String,
    },
    RestartMcp(String),
    /// Opens the OAuth sign-in of a remote MCP server in the browser.
    SignInMcp(String),
    Resend(String),
    /// A command the core answers for the client, such as the threads for /resume.
    Command(Command),
    OpenThread(String),
    /// Download a release and put it in place of the running binary.
    InstallUpdate(String),
    /// Move the chat to its next mode.
    NextMode,
    /// Move the chat to the mode named, as a Telegram bot asked.
    SetMode {
        bot: usize,
        name: String,
    },
    /// Send a Telegram bot the menu of what its command can be set to.
    BotMenu {
        bot: usize,
        menu: BotMenu,
    },
    /// Stop one subagent and the agents it asked.
    StopAgent(String),
    Copy(String),
    /// Open the window of the subagent working in this thread.
    OpenAgent(String),
    /// Load the thread the threads panel points at, for its preview.
    Preview(String),
}

/// Every command the box offers, as the core lists them.
pub fn commands() -> impl Iterator<Item = &'static (&'static str, &'static str)> {
    const LOCAL: &[(&str, &str)] = &[
        (
            "/connect",
            "add a provider with its URL and optional API key",
        ),
        (
            "/resend",
            "view local unsent messages and explicitly send one",
        ),
        (
            "/tgbot",
            "mirror this chat in a Telegram bot: /tgbot <token>; alone, the bots",
        ),
    ];
    COMMANDS.iter().chain(LOCAL.iter())
}

/// What a Telegram bot's menu lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BotMenu {
    Modes,
    Models,
}
