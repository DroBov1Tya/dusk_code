//! Commands a client sends as a message, and what the core answers to the ones that do
//! not run in the thread.

use std::path::PathBuf;

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

use crate::{
    context::ContextReport,
    conversation::{Source, UndoPoint},
    model::Effort,
    providers::ProviderStatus,
    settings::Settings,
    thread::Thread,
    update::UpdateInfo,
    work::JobInfo,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Command {
    /// Writes a portable archive of config and chat data; never sends it anywhere.
    Migrate { path: Option<String> },
    /// Summarizes the context so far; the argument is extra instructions for the summary.
    Compact { instructions: Option<String> },
    /// Lists the threads to open again; a client fills in the folder it runs in.
    Resume { cwd: Option<PathBuf> },
    /// Leaves the open thread for a new one, created with its first message.
    New,
    /// Lists the providers with their models, or switches to `provider/model`.
    Model { name: Option<String> },
    /// What a client offers to switch: tools, skills, agents and limits.
    Settings,
    /// What the next request of the thread carries; a client fills in its open thread.
    Context { thread_id: Option<String> },
    /// Describes the thread in detail and goes on in a new thread from that; the
    /// argument is extra instructions for the description.
    Fork { instructions: Option<String> },
    /// The newest version on the update channel; `dev` or `stable` switches it first.
    Update { channel: Option<String> },
    /// Shows the reasoning effort, or sets it to a supported level or default.
    Effort { level: Option<String> },
    /// Creates a draft goal; shows, approves, confirms, pauses, resumes or clears it.
    Goal { argument: Option<String> },
    /// Lists the user's messages of a thread; a client fills in its open thread. With a
    /// sequence, takes the thread back to before that message.
    Undo {
        thread_id: Option<String>,
        sequence: Option<u64>,
    },
    /// Lists a thread's sources, after copying the file at `add` among them; a client
    /// fills in its open thread.
    Sources {
        thread_id: Option<String>,
        add: Option<String>,
    },
    /// Lists the background jobs of a thread; a client fills in its open thread.
    Jobs { thread_id: Option<String> },
}

impl Command {
    /// Thread commands go to the thread as a message; the rest go to `answer`.
    pub fn runs_in_thread(&self) -> bool {
        matches!(
            self,
            Self::Compact { .. }
                | Self::Fork { .. }
                | Self::Goal { .. }
                | Self::Undo {
                    sequence: Some(_),
                    ..
                }
        )
    }
}

/// What a client command answers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Answer {
    /// The archive contains config secrets as well as chat history.
    Migrated(PathBuf),
    Threads(Vec<Thread>),
    /// Every configured provider, checked just now, and the model in use.
    Providers {
        current: Option<String>,
        providers: Vec<ProviderStatus>,
    },
    /// Every thread talks to this model from now on.
    ModelSwitched {
        name: String,
    },
    /// The client closes the open thread and shows an empty one.
    NewThread,
    Settings(Box<Settings>),
    Context(ContextReport),
    Update(UpdateInfo),
    /// The user's messages the thread can go back to, oldest first.
    UndoPoints(Vec<UndoPoint>),
    /// The effort now; `changed` when the command set it.
    Effort {
        current: Option<Effort>,
        changed: bool,
    },
    /// The thread's background jobs, oldest first.
    Jobs(Vec<JobInfo>),
    /// The thread's sources, oldest first.
    Sources(Vec<Source>),
}

/// Every command, for clients to show: name and what it does.
pub const COMMANDS: &[(&str, &str)] = &[
    (
        "/migrate",
        "save config and chats, including API keys; optional output path must be inside ~/Documents/duskcode/backups",
    ),
    (
        "/compact",
        "summarize the conversation to free the context; text after it guides the summary",
    ),
    ("/new", "start a new thread"),
    (
        "/fork",
        "go on in a new thread from a detailed summary of this one",
    ),
    ("/resume", "open an earlier thread"),
    ("/model", "choose the model; every provider is checked live"),
    ("/settings", "switch agents, tools and skills; edit skills"),
    (
        "/context",
        "what the context holds: prompt, tools, messages",
    ),
    (
        "/effort",
        "how hard the model reasons: off, low, medium, high, xhigh, max or default",
    ),
    ("/update", "the newest version, and installing it"),
    (
        "/undo",
        "take the thread back to before one of your messages",
    ),
    (
        "/goal",
        "a verified goal: review criteria, approve <revision>, confirm evidence; pause, resume, clear",
    ),
    (
        "/jobs",
        "background commands: their state and output; stop one",
    ),
    (
        "/sources",
        "files attached to this chat: pin or delete; a path adds a file",
    ),
];

/// `None` for an ordinary message; an error for a `/word` that is not a command.
pub fn parse(text: &str) -> Option<Result<Command>> {
    let text = text.trim_start();
    let rest = text.strip_prefix('/')?;
    let (name, argument) = match rest.split_once(char::is_whitespace) {
        Some((name, argument)) => (name, argument.trim()),
        None => (rest, ""),
    };
    if name.is_empty() || !name.chars().all(|c| c.is_ascii_lowercase() || c == '-') {
        return None;
    }
    let argument = (!argument.is_empty()).then(|| argument.to_owned());
    Some(match name {
        "migrate" => Ok(Command::Migrate { path: argument }),
        "compact" => Ok(Command::Compact {
            instructions: argument,
        }),
        "fork" => Ok(Command::Fork {
            instructions: argument,
        }),
        "resume" => Ok(Command::Resume { cwd: None }),
        "new" => Ok(Command::New),
        "settings" => Ok(Command::Settings),
        "context" => Ok(Command::Context { thread_id: None }),
        "effort" => Ok(Command::Effort { level: argument }),
        "update" => Ok(Command::Update { channel: argument }),
        "goal" => Ok(Command::Goal { argument }),
        "undo" => match argument.map(|number| number.parse::<u64>()) {
            None => Ok(Command::Undo {
                thread_id: None,
                sequence: None,
            }),
            Some(Ok(sequence)) => Ok(Command::Undo {
                thread_id: None,
                sequence: Some(sequence),
            }),
            Some(Err(_)) => Err(anyhow::anyhow!("/undo takes a message number")),
        },
        "model" => Ok(Command::Model { name: argument }),
        "jobs" => Ok(Command::Jobs { thread_id: None }),
        "sources" => Ok(Command::Sources {
            thread_id: None,
            add: argument,
        }),
        _ => unknown(name),
    })
}

fn unknown(name: &str) -> Result<Command> {
    bail!("unknown command /{name}")
}
