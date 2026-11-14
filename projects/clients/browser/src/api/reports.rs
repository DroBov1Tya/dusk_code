//! The core's requests as typed calls, so a view says what it wants and not which
//! response variant carries it. Every error is the core's own text.

use crate::link::request;
use dusk_protocol::{
    client::{Answer, Command, Request, Response},
    config::Settings,
    system::SystemReport,
    thread::ContextReport,
};

use super::*;

pub async fn command(command: Command) -> Res<Answer> {
    ask!(Request::Command(command), Response::Answer(a) => *a)
}

pub async fn settings_panel() -> Res<Settings> {
    match command(Command::Settings).await? {
        Answer::Settings(panel) => Ok(*panel),
        other => Err(format!("The core answered {other:?}, not the settings.")),
    }
}

pub async fn context_report(thread_id: Option<String>) -> Res<ContextReport> {
    match command(Command::Context { thread_id }).await? {
        Answer::Context(report) => Ok(report),
        other => Err(format!("The core answered {other:?}, not a report.")),
    }
}

pub async fn system_report() -> Res<SystemReport> {
    ask!(Request::SystemReport, Response::System(r) => r)
}

pub async fn install_update(tag: String) -> Res<String> {
    ask!(Request::InstallUpdate { tag }, Response::Path(p) => p.display().to_string())
}
