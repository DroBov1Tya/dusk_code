//! Reading a message as a command.

use anyhow::{Result, bail};

use crate::client::Command;
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
    ("/sessions", "the same as /resume"),
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
        "resume" | "sessions" => Ok(Command::Resume { cwd: None }),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sessions_is_resume() {
        for text in ["/resume", "/sessions", "  /sessions"] {
            assert!(matches!(
                parse(text).unwrap().unwrap(),
                Command::Resume { cwd: None }
            ));
        }
        assert!(parse("/session").unwrap().is_err());
    }
}
