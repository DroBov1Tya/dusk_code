//! Commands a client sends as a message, and what the core answers to the ones that do
//! not run in a thread.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::config::Settings;
use crate::model::{Effort, ProviderStatus};
use crate::release::UpdateInfo;
use crate::thread::{ContextReport, JobInfo, Source, Thread, UndoPoint};
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

mod parse;

pub use parse::{COMMANDS, parse};
