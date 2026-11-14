//! The timetable from the terminal: listing, adding in the project of this folder,
//! running now, switching on or off, removing.

use anyhow::{Context, Result};
use dusk_core::client::Client;
use dusk_protocol::project::{Schedule, ScheduleDraft};

use crate::app::{App, Panel, schedule::ScheduleAction};

pub async fn apply(client: &Client, app: &mut App, action: ScheduleAction) {
    let projects = client.projects();
    let result: Result<Option<Vec<Schedule>>> = async {
        Ok(match action {
            ScheduleAction::Open => Some(projects.schedules().await?),
            ScheduleAction::Add { repeat, prompt } => {
                let project_id = this_project(client).await?;
                let tasks = projects
                    .save_schedule(ScheduleDraft {
                        id: None,
                        project_id,
                        prompt,
                        repeat,
                        enabled: true,
                        model: None,
                    })
                    .await?;
                app.flash = Some("Task scheduled; /schedules lists them".into());
                Some(tasks)
            }
            ScheduleAction::Run(id) => {
                projects.run_schedule(id).await?;
                app.flash = Some("The task runs now in a new chat; /resume shows it".into());
                Some(projects.schedules().await?)
            }
            ScheduleAction::Delete(id) => Some(projects.delete_schedule(id).await?),
            ScheduleAction::Toggle { id, on } => {
                let tasks = projects.schedules().await?;
                let task = tasks
                    .into_iter()
                    .find(|t| t.id == id)
                    .context("no such task")?;
                Some(
                    projects
                        .save_schedule(ScheduleDraft {
                            id: Some(task.id),
                            project_id: task.project_id,
                            prompt: task.prompt,
                            repeat: task.repeat,
                            enabled: on,
                            model: task.model,
                        })
                        .await?,
                )
            }
        })
    }
    .await;
    match result {
        Ok(Some(tasks)) => show(app, tasks),
        Ok(None) => {}
        Err(error) => app.error(format!("{error:#}")),
    }
}

/// The saved project whose folder this terminal is in; a task runs only in one.
async fn this_project(client: &Client) -> Result<String> {
    let here = std::env::current_dir()?.canonicalize()?;
    client
        .projects()
        .list()
        .await?
        .into_iter()
        .find(|project| here.starts_with(&project.dir))
        .map(|project| project.id)
        .context("this folder is not a saved project; a task runs only in one")
}

fn show(app: &mut App, tasks: Vec<Schedule>) {
    let selected = match &app.panel {
        Some(Panel::Schedules { selected, .. }) => (*selected).min(tasks.len().saturating_sub(1)),
        _ => 0,
    };
    app.panel = Some(Panel::Schedules {
        tasks,
        selected,
        removing: false,
    });
}
