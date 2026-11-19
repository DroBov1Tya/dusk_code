//! What the app asks the session to do, and the commands the box offers.

use dusk_protocol::{
    client::{COMMANDS, Command},
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
        thread_id: String,
        id: u64,
        decision: Decision,
    },
    /// Switches the plugin named on or off.
    TogglePlugin(String),
    /// Keeps a plugin's secret setting for this session; it is never saved.
    SetPluginSecret {
        plugin: String,
        key: String,
        value: String,
    },
    /// Reads the store and the skill packs into the Workshop.
    LoadStore,
    /// Installs or updates a plugin from the store at the version the user saw.
    InstallPlugin(Box<dusk_protocol::plugin::Listing>),
    /// Removes a plugin the store installed.
    RemovePlugin(String),
    /// Updates what asks for nothing more, and says what was done and left.
    UpdateAll,
    /// Switches a skill pack or a skill of one from the Workshop.
    PackSwitch(dusk_protocol::client::Switch, bool),
    Connect {
        kind: ProviderKind,
        /// `None` takes the next free `provider-N`.
        name: Option<String>,
        base_url: String,
        api_key: Option<String>,
    },
    SetAgentLimit(usize),
    SetGoalRounds(u32),
    SetModelTimeout(u32),
    SetMaxAttempts(u32),
    SetIdleMinutes(u32),
    SetBranchPrefix(String),
    /// `/plan`: open the chat's pinned plan in the editor.
    OpenPlan,
    /// Read the Overview and open its panel.
    Overview,
    /// `/files`: what this chat's commands changed in the project.
    ChangedFiles,
    /// `/diff`: the folder's changes line by line.
    Diff,
    /// `/me`: what `@me` says about the user, in the editor.
    OpenMe,
    /// `/snippet name`: a snippet for `@name`, new or as it is.
    OpenSnippet(String),
    SaveMe(String),
    SaveSnippet(String, String),
    /// Switches a provider on or off; its key stays.
    ProviderEnabled(String, bool),
    /// `/disconnect name`: forget a provider.
    Disconnect(String),
    /// The plan as the editor saved it; empty text removes it.
    SavePlan(String),
    /// `/pins`, `/pin` and `/unpin N`: the chat's pinned messages.
    Pin(PinAsk),
    /// `/access`: this chat's own approval mode, or the one it has now when none is named.
    Access(Option<String>),
    SetMemoryChars(usize),
    SetWorkHistoryChars(usize),
    PinSource {
        id: String,
        pinned: bool,
    },
    RemoveSource(String),
    /// `/schedule`, `/schedules` and the keys of their panel.
    Schedule(crate::app::schedule::ScheduleAction),
    /// Ask a running background job of the open thread to stop.
    StopJob(String),
    /// Send the agent's running command, by its tool call, to the background.
    Background(String),
    /// Open, close or switch a tab.
    Tab(super::TabMove),
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
    /// Open OpenAI's sign-in page; the provider is saved once the browser comes back.
    SignInChatGpt,
    Resend(String),
    /// A command the core answers for the client, such as the threads for /resume.
    Command(Command),
    OpenThread(String),
    /// Download a release and put it in place of the running binary.
    InstallUpdate(String),
    /// Open a page in the system's browser.
    OpenLink(String),
    /// Move the chat to its next mode.
    NextMode,
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
            "/plugins",
            "the plugins found, to switch on or off; /plugins store installs more",
        ),
        (
            "/tab",
            "a new tab; /tab 2 shows tab 2, /tab next, /tab prev, /tab close",
        ),
        ("/plan", "read and edit this chat's pinned plan"),
        (
            "/overview",
            "how the chat's work stands: plan, tokens, changes, activity, log",
        ),
        ("/pins", "this chat's pinned messages, numbered"),
        (
            "/pin",
            "pin the agent's last answer; /unpin N takes pin N away",
        ),
        ("/files", "the files this chat's commands changed"),
        (
            "/link",
            "copy this chat's link, for a message in another chat",
        ),
        ("/diff", "review the changes in this chat's folder"),
        (
            "/disconnect",
            "forget a provider and its key: /disconnect name",
        ),
        (
            "/me",
            "what @me says about you; its files are set in the GUI",
        ),
        ("/snippet", "write a snippet for @name: /snippet name"),
        (
            "/access",
            "this chat's access: ask, sandbox, auto, full or default",
        ),
    ];
    COMMANDS.iter().chain(LOCAL.iter())
}

/// What a pin command asks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PinAsk {
    /// Lists the pins, numbered.
    List,
    /// Pins the agent's last answer.
    Last,
    /// Unpins the pin of this number, from 1.
    Unpin(usize),
}
