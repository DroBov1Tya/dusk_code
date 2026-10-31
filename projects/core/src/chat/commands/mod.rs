//! Commands a client sends as a message: a known `/word` at the very start, optionally
//! followed by an argument. Anything else is an ordinary message, `/etc/hosts` included.
//!
//! Each command has a file of its own with what it does. Thread commands run in the
//! thread, in order with its messages; client commands answer the client at once, which
//! only shows the answer.

mod compact;
mod context;
mod goal;
mod model;
mod resume;
mod settings;
mod undo;

pub use context::{ContextReport, ContextSection, report as context_report};
pub use settings::{Settings, ToolInfo};

use std::path::PathBuf;

use anyhow::{Context, Result, bail};

use crate::{
    config::Effort,
    release::update::{Channel, UpdateInfo},
};

use crate::{
    runtime::{
        Runtime,
        agent::{Agent, Inbox},
        model_choice::{Choice, ProviderStatus},
    },
    state::{Source, Thread, UndoPoint},
    tools::JobInfo,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
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
    /// Shows the reasoning effort, or sets it: low, medium, high or default.
    Effort { level: Option<String> },
    /// Sets the standing goal, or shows, pauses, resumes, clears or completes it.
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

/// What a client command answers.
pub enum Answer {
    Threads(Vec<Thread>),
    /// Every configured provider, checked just now, and the model in use.
    Providers {
        current: Option<String>,
        providers: Vec<ProviderStatus>,
    },
    /// Every thread talks to this model from now on.
    ModelSwitched(Choice),
    /// The client closes the open thread and shows an empty one.
    NewThread,
    Settings(Settings),
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
        "a goal worked on turn after turn until done; pause, resume, clear",
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

    pub async fn run_in_thread(self, agent: &Agent, inbox: &mut Inbox) {
        match self {
            Self::Compact { instructions } => {
                compact::run(agent, instructions.as_deref(), inbox).await;
            }
            Self::Fork { instructions } => {
                // A stop only ends the fork; this thread goes on as it was.
                let _ = agent.fork(instructions.as_deref(), inbox).await;
            }
            Self::Goal { argument } => goal::run(agent, argument.as_deref()).await,
            Self::Undo {
                sequence: Some(sequence),
                ..
            } => undo::run(agent, sequence).await,
            Self::Undo { sequence: None, .. } => {}
            Self::Resume { .. }
            | Self::New
            | Self::Model { .. }
            | Self::Settings
            | Self::Context { .. }
            | Self::Effort { .. }
            | Self::Jobs { .. }
            | Self::Sources { .. }
            | Self::Update { .. } => {}
        }
    }

    pub async fn answer(self, runtime: &Runtime) -> Result<Answer> {
        match self {
            Self::Resume { cwd } => Ok(Answer::Threads(resume::run(runtime, cwd).await?)),
            Self::New => Ok(Answer::NewThread),
            Self::Settings => Ok(Answer::Settings(settings::run(runtime).await?)),
            Self::Update { channel } => {
                let channel = match channel.as_deref() {
                    None => None,
                    Some("stable") => Some(Channel::Stable),
                    Some("dev") => Some(Channel::Dev),
                    Some(other) => bail!("unknown channel {other}; stable or dev"),
                };
                Ok(Answer::Update(runtime.check_update(channel).await?))
            }
            Self::Effort { level: None } => Ok(Answer::Effort {
                current: runtime.config().current().effort(),
                changed: false,
            }),
            Self::Effort { level: Some(level) } => {
                let effort = Effort::parse(&level)?;
                runtime
                    .config()
                    .update(|config| config.set_effort(effort))?;
                Ok(Answer::Effort {
                    current: effort,
                    changed: true,
                })
            }
            Self::Context { thread_id } => Ok(Answer::Context(
                runtime.context(thread_id.as_deref()).await?,
            )),
            Self::Model { name: None } => Ok(Answer::Providers {
                current: runtime.model().map(|choice| choice.name()),
                providers: model::check(runtime).await?,
            }),
            Self::Model { name: Some(name) } => {
                Ok(Answer::ModelSwitched(model::switch(runtime, &name).await?))
            }
            Self::Compact { .. } => bail!("/compact runs in a thread"),
            Self::Fork { .. } => bail!("/fork runs in a thread"),
            Self::Goal { .. } => bail!("/goal runs in a thread"),
            Self::Undo {
                thread_id,
                sequence: None,
            } => {
                let thread_id = thread_id.context("no thread to undo in")?;
                Ok(Answer::UndoPoints(runtime.undo_points(&thread_id).await?))
            }
            Self::Undo { .. } => bail!("/undo <n> runs in a thread"),
            Self::Sources {
                thread_id: None, ..
            } => Ok(Answer::Sources(Vec::new())),
            Self::Sources {
                thread_id: Some(thread_id),
                add,
            } => {
                if let Some(path) = add {
                    runtime.add_source(&thread_id, &expand_home(&path)).await?;
                }
                Ok(Answer::Sources(runtime.sources(&thread_id).await?))
            }
            Self::Jobs { thread_id: None } => Ok(Answer::Jobs(Vec::new())),
            Self::Jobs {
                thread_id: Some(thread_id),
            } => Ok(Answer::Jobs(runtime.jobs(&thread_id).await?)),
        }
    }
}

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

/// A leading `~/` as people write it.
fn expand_home(path: &str) -> PathBuf {
    match (path.strip_prefix("~/"), std::env::var_os("HOME")) {
        (Some(rest), Some(home)) => PathBuf::from(home).join(rest),
        _ => PathBuf::from(path),
    }
}

fn unknown(name: &str) -> Result<Command> {
    bail!("unknown command /{name}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordinary_text_is_not_a_command() {
        for text in [
            "hello /compact",
            "/etc/hosts is empty",
            "/",
            "/ compact",
            "/Compact",
        ] {
            assert!(parse(text).is_none(), "{text:?}");
        }
    }
}
