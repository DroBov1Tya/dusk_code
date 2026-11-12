//! What the settings change.

use serde::{Deserialize, Serialize};

use crate::client::Secret;
use crate::config::{ApprovalPolicy, ContextPolicy, ProviderKind, VoiceSettings};
/// What `/settings` changes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SettingsRequest {
    /// Answers with the input limit it gives.
    ContextPolicy(ContextPolicy),
    MaxAgentsAtWork(usize),
    /// Answers with the minutes saved, after the config's bounds.
    IdleScreenMinutes(u32),
    MaxGoalRounds(u32),
    MemoryChars(usize),
    /// Answers with the characters saved, after the config's bounds.
    WorkHistoryChars(usize),
    Switch {
        switch: Switch,
        on: bool,
    },
    /// The version control branch prefix; empty adds none.
    BranchPrefix(String),
    /// Answers with the mode switched to.
    NextMode,
    /// Switches to the mode of this name.
    Mode(String),
    /// Saves a provider; a model is chosen with `/model`.
    Connect {
        kind: ProviderKind,
        name: Option<String>,
        base_url: String,
        api_key: Option<Secret>,
    },
    /// Asks a provider for its models and balance, to see whether it answers; nothing is
    /// saved. Answers with `Response::Provider`.
    CheckProvider(ProviderTarget),
    /// Rebuilds a saved provider's connection, with a new key when one is given, and checks
    /// it. Answers with `Response::Provider`.
    ReconnectProvider {
        name: String,
        api_key: Option<Secret>,
    },
    /// Forgets a provider. Refused while its model is the chosen one.
    RemoveProvider {
        name: String,
    },
    /// Switches a saved provider on or off; its key stays. Off is refused while its model
    /// is the chosen one.
    ProviderEnabled {
        name: String,
        enabled: bool,
    },
    /// Forgets every provider and the chosen model.
    RemoveAllProviders,
    /// Keeps a secret in the system keychain; only the names the core knows are taken.
    SetSecret {
        name: String,
        value: Secret,
    },
    DeleteSecret {
        name: String,
    },
    /// Saves the speech servers; their keys go through `SetSecret`.
    Voice(VoiceSettings),
    PluginEnabled {
        name: String,
        enabled: bool,
    },
    /// Kept in memory for this run only, never in the config file.
    PluginSecret {
        plugin: String,
        key: String,
        value: Secret,
    },
}

/// What a switch in the settings turns on or off.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Switch {
    Agent(String),
    Tool(String),
    Skill(String),
    Mcp(String),
    Memory,
    WorkHistory,
    DevChannel,
    /// On sets the policy, off goes back to asking.
    Approvals(ApprovalPolicy),
    Logging,
    /// Whether clients notify at all.
    Notifications,
    /// Whether notifications ring the terminal bell.
    SoundNotifications,
}

/// Which provider a check asks: one typed in a form and not saved yet, or a saved one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProviderTarget {
    New {
        kind: ProviderKind,
        base_url: String,
        api_key: Option<Secret>,
    },
    Saved {
        name: String,
    },
}
