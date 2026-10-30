//! Commands a client sends as a message: a known `/word` at the very start, optionally
//! followed by an argument. Anything else is an ordinary message, `/etc/hosts` included.
//!
//! Each command has a file of its own with what it does. Thread commands run in the
//! thread, in order with its messages; client commands answer the client at once, which
//! only shows the answer.

mod compact;
mod model;
mod resume;

use anyhow::{Result, bail};

use crate::{
    runtime::{
        Runtime,
        agent::Agent,
        inbox::Inbox,
        model_choice::{Choice, ProviderStatus},
    },
    state::Thread,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Summarizes the context so far; the argument is extra instructions for the summary.
    Compact { instructions: Option<String> },
    /// Lists the threads to open again.
    Resume,
    /// Leaves the open thread for a new one, created with its first message.
    New,
    /// Lists the providers with their models, or switches to `provider/model`.
    Model { name: Option<String> },
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
}

/// Every command, for clients to show: name and what it does.
pub const COMMANDS: &[(&str, &str)] = &[
    (
        "/compact",
        "summarize the conversation to free the context; text after it guides the summary",
    ),
    ("/new", "start a new thread"),
    ("/resume", "open an earlier thread"),
    ("/model", "choose the model; every provider is checked live"),
];

impl Command {
    /// Thread commands go to the thread as a message; the rest go to `answer`.
    pub fn runs_in_thread(&self) -> bool {
        matches!(self, Self::Compact { .. })
    }

    pub async fn run_in_thread(self, agent: &Agent, inbox: &mut Inbox) {
        match self {
            Self::Compact { instructions } => {
                compact::run(agent, instructions.as_deref(), inbox).await;
            }
            Self::Resume | Self::New | Self::Model { .. } => {}
        }
    }

    pub async fn answer(self, runtime: &Runtime) -> Result<Answer> {
        match self {
            Self::Resume => Ok(Answer::Threads(resume::run(runtime).await?)),
            Self::New => Ok(Answer::NewThread),
            Self::Model { name: None } => Ok(Answer::Providers {
                current: runtime.model().map(|choice| choice.name()),
                providers: model::check(runtime).await?,
            }),
            Self::Model { name: Some(name) } => {
                Ok(Answer::ModelSwitched(model::switch(runtime, &name).await?))
            }
            Self::Compact { .. } => bail!("/compact runs in a thread"),
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
        "resume" => Ok(Command::Resume),
        "new" => Ok(Command::New),
        "model" => Ok(Command::Model { name: argument }),
        _ => unknown(name),
    })
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
