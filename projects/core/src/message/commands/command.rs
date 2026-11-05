//! What a command is: its parsing from a message, and its answer for a client.

use std::path::PathBuf;

use anyhow::{Context, Result, bail};

use crate::{config::Effort, release::update::Channel};

use crate::runtime::{
    Runtime,
    agent::{Agent, Inbox},
};

pub use dusk_protocol::command::{Answer, Command, parse};

use super::{compact, goal, model, resume, settings, undo};

/// Runs a thread command in its thread, in order with the thread's messages.
pub async fn run_in_thread(command: Command, agent: &Agent, inbox: &mut Inbox) {
    match command {
        Command::Compact { instructions } => {
            compact::run(agent, instructions.as_deref(), inbox).await;
        }
        Command::Fork { instructions } => {
            // A stop only ends the fork; this thread goes on as it was.
            let _ = agent.fork(instructions.as_deref(), inbox).await;
        }
        Command::Goal { argument } => goal::run(agent, argument.as_deref()).await,
        Command::Undo {
            sequence: Some(sequence),
            ..
        } => undo::run(agent, sequence).await,
        Command::Undo { sequence: None, .. } => {}
        Command::Resume { .. }
        | Command::Migrate { .. }
        | Command::New
        | Command::Model { .. }
        | Command::Settings
        | Command::Context { .. }
        | Command::Effort { .. }
        | Command::Jobs { .. }
        | Command::Sources { .. }
        | Command::Update { .. } => {}
    }
}

/// What a client command answers.
pub async fn answer(command: Command, runtime: &Runtime) -> Result<Answer> {
    match command {
        Command::Migrate { path } => Ok(Answer::Migrated(
            runtime.migrate(path.as_deref().map(expand_home)).await?,
        )),
        Command::Resume { cwd } => Ok(Answer::Threads(resume::run(runtime, cwd).await?)),
        Command::New => Ok(Answer::NewThread),
        Command::Settings => Ok(Answer::Settings(Box::new(settings::run(runtime).await?))),
        Command::Update { channel } => {
            let channel = match channel.as_deref() {
                None => None,
                Some("stable") => Some(Channel::Stable),
                Some("dev") => Some(Channel::Dev),
                Some(other) => bail!("unknown channel {other}; stable or dev"),
            };
            Ok(Answer::Update(runtime.check_update(channel).await?))
        }
        Command::Effort { level: None } => Ok(Answer::Effort {
            current: runtime.config().current().effort(),
            changed: false,
        }),
        Command::Effort { level: Some(level) } => {
            let effort = Effort::parse(&level)?;
            runtime
                .config()
                .update(|config| config.set_effort(effort))?;
            Ok(Answer::Effort {
                current: effort,
                changed: true,
            })
        }
        Command::Context { thread_id } => Ok(Answer::Context(
            runtime.context(thread_id.as_deref()).await?,
        )),
        Command::Model { name: None } => Ok(Answer::Providers {
            current: runtime.model().map(|choice| choice.name()),
            providers: model::check(runtime).await?,
        }),
        Command::Model { name: Some(name) } => Ok(Answer::ModelSwitched {
            name: model::switch(runtime, &name).await?.name(),
        }),
        Command::Compact { .. } => bail!("/compact runs in a thread"),
        Command::Fork { .. } => bail!("/fork runs in a thread"),
        Command::Goal { .. } => bail!("/goal runs in a thread"),
        Command::Undo {
            thread_id,
            sequence: None,
        } => {
            let thread_id = thread_id.context("no thread to undo in")?;
            Ok(Answer::UndoPoints(runtime.undo_points(&thread_id).await?))
        }
        Command::Undo { .. } => bail!("/undo <n> runs in a thread"),
        Command::Sources {
            thread_id: None, ..
        } => Ok(Answer::Sources(Vec::new())),
        Command::Sources {
            thread_id: Some(thread_id),
            add,
        } => {
            if let Some(path) = add {
                runtime.add_source(&thread_id, &expand_home(&path)).await?;
            }
            Ok(Answer::Sources(runtime.sources(&thread_id).await?))
        }
        Command::Jobs { thread_id: None } => Ok(Answer::Jobs(Vec::new())),
        Command::Jobs {
            thread_id: Some(thread_id),
        } => Ok(Answer::Jobs(runtime.jobs(&thread_id).await?)),
    }
}

/// A leading `~/` as people write it.
fn expand_home(path: &str) -> PathBuf {
    match (path.strip_prefix("~/"), std::env::var_os("HOME")) {
        (Some(rest), Some(home)) => PathBuf::from(home).join(rest),
        _ => PathBuf::from(path),
    }
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
