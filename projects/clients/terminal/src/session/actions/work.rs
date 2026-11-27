//! Actions on what a thread works on: its plan, changes, sources and jobs.

use dusk_core::client::Client;
use dusk_protocol::client::Command;

use crate::app::{App, DiffView, OverviewView, Panel, PasteEditor};
use crate::session::{
    answers::answer,
    thread::{OpenThread, ensure_open},
};

pub async fn open_plan(client: &Client, app: &mut App) {
    let Some(thread_id) = app.thread_id.clone() else {
        app.flash = Some("Send a message first; the plan belongs to a chat".into());
        return;
    };
    match client.work().plan(&thread_id).await {
        Ok(plan) => {
            let text = plan.unwrap_or_default();
            app.panel = Some(Panel::Paste(PasteEditor::plan(text)));
        }
        Err(error) => app.error(format!("cannot read the plan: {error:#}")),
    }
}

/// Opens the Overview; the clock keeps it current while it is open.
pub async fn overview(client: &Client, app: &mut App) {
    let Some(thread_id) = app.thread_id.clone() else {
        app.flash = Some("Send a message first; the Overview belongs to a chat".into());
        return;
    };
    match crate::session::answers::read_overview(client, &thread_id).await {
        Ok((overview, context)) => {
            app.panel = Some(Panel::Overview(Box::new(OverviewView::new(
                overview, context,
            ))));
            app.overview_stale = true;
        }
        Err(error) => app.error(format!("cannot read the Overview: {error:#}")),
    }
}

/// Stops the chat's running team; its events move the plan, the clock reads the team.
pub async fn stop_team(client: &Client, app: &mut App) {
    let Some(thread_id) = app.thread_id.clone() else {
        return;
    };
    match client.work().stop_team(&thread_id).await {
        Ok(()) => {
            app.flash = Some("Team stopped; its tasks at work went back to open".into());
            app.overview_stale = true;
        }
        Err(error) => app.error(format!("cannot stop the team: {error:#}")),
    }
}

/// Retries failed tasks of the chat's plan in place; their own events move the plan.
pub async fn retry_tasks(client: &Client, app: &mut App, ids: Vec<String>) {
    let Some(thread_id) = app.thread_id.clone() else {
        return;
    };
    for task_id in &ids {
        if let Err(error) = client.work().retry_task(&thread_id, task_id).await {
            return app.error(format!("cannot retry the task: {error:#}"));
        }
    }
    app.flash = Some(match ids.len() {
        1 => "Retrying the failed task".into(),
        count => format!("Retrying {count} failed tasks"),
    });
    app.overview_stale = true;
}

/// Lets `at_once` members of the chat's running team work at once; a refusal stays under
/// the stepper.
pub async fn set_team_at_once(client: &Client, app: &mut App, at_once: u32) {
    let Some(thread_id) = app.thread_id.clone() else {
        return;
    };
    let result = client.work().set_team_at_once(&thread_id, at_once).await;
    let Some(Panel::Overview(view)) = &mut app.panel else {
        return;
    };
    match result {
        Ok(()) => {
            view.at_once = None;
            app.flash = Some(format!("{at_once} members at once"));
            app.overview_stale = true;
        }
        Err(error) => {
            if let Some(edit) = &mut view.at_once {
                edit.error = Some(format!("{error:#}"));
            }
        }
    }
}

pub async fn save_plan(client: &Client, app: &mut App, text: String) {
    if let Some(thread_id) = app.thread_id.clone() {
        match client.work().save_plan(&thread_id, &text).await {
            Ok(()) => {
                app.flash = Some(match text.trim().is_empty() {
                    true => "Plan removed".to_owned(),
                    false => "Plan saved; the model reads it with every message".to_owned(),
                })
            }
            Err(error) => app.error(format!("cannot save the plan: {error:#}")),
        }
    }
}

pub async fn diff(client: &Client, app: &mut App) {
    let Some(thread_id) = app.thread_id.clone() else {
        app.flash = Some("Send a message first; the changes belong to a chat".into());
        return;
    };
    match client.work().diff(&thread_id, None).await {
        Ok(files) if files.is_empty() => {
            app.flash = Some("No changes: the folder matches the last commit".into())
        }
        Ok(files) => {
            app.panel = Some(Panel::Diff(DiffView {
                files,
                selected: 0,
                open: false,
                scroll: 0,
            }))
        }
        Err(error) => app.error(format!("cannot read the changes: {error:#}")),
    }
}

pub async fn changed_files(client: &Client, app: &mut App) {
    let Some(thread_id) = app.thread_id.clone() else {
        app.flash = Some("Send a message first; the files belong to a chat".into());
        return;
    };
    match client.work().changed_files(&thread_id).await {
        Ok(files) if files.is_empty() => {
            app.flash = Some("No commands of this chat changed a file yet".into())
        }
        Ok(files) => {
            let shown: Vec<_> = files.iter().take(5).map(|f| f.path.as_str()).collect();
            let more = files.len() - shown.len();
            let tail = if more > 0 {
                format!(" +{more}")
            } else {
                String::new()
            };
            app.flash = Some(format!("Changed: {}{tail}", shown.join(", ")));
        }
        Err(error) => app.error(format!("cannot read the files: {error:#}")),
    }
}

pub async fn sources(
    client: &Client,
    app: &mut App,
    open: &mut Option<OpenThread>,
    add: Option<String>,
) {
    // A file can be the first thing a chat gets.
    if add.is_some()
        && let Err(error) = ensure_open(client, app, open).await
    {
        app.error(format!("cannot open a thread: {error:#}"));
        return;
    }
    let thread_id = app.thread_id.clone();
    let command = Command::Sources { thread_id, add };
    answer(client, app, open, command).await;
}

pub async fn pin_source(
    client: &Client,
    app: &mut App,
    open: &mut Option<OpenThread>,
    id: String,
    pinned: bool,
) {
    if let Some(thread_id) = app.thread_id.clone() {
        if let Err(error) = client.work().pin_source(&thread_id, &id, pinned).await {
            app.error(format!("cannot pin the source: {error:#}"));
        }
        let command = Command::Sources {
            thread_id: Some(thread_id),
            add: None,
        };
        answer(client, app, open, command).await;
    }
}

pub async fn remove_source(
    client: &Client,
    app: &mut App,
    open: &mut Option<OpenThread>,
    id: String,
) {
    if let Some(thread_id) = app.thread_id.clone() {
        if let Err(error) = client.work().remove_source(&thread_id, &id).await {
            app.error(format!("cannot remove the source: {error:#}"));
        }
        let command = Command::Sources {
            thread_id: Some(thread_id),
            add: None,
        };
        answer(client, app, open, command).await;
    }
}

pub async fn jobs(client: &Client, app: &mut App, open: &mut Option<OpenThread>) {
    let thread_id = app.thread_id.clone();
    answer(client, app, open, Command::Jobs { thread_id }).await;
}

pub async fn background(client: &Client, app: &mut App, call_id: String) {
    if let Some(thread_id) = app.thread_id.clone() {
        match client
            .threads()
            .send_to_background(&thread_id, &call_id)
            .await
        {
            Ok(()) => app.flash = Some("the command goes on in the background".into()),
            Err(error) => app.error(format!("cannot send it to the background: {error:#}")),
        }
        app.jobs_stale = true;
    }
}

pub async fn stop_job(client: &Client, app: &mut App, job_id: String) {
    if let Some(thread_id) = app.thread_id.clone() {
        match client.work().stop_job(&thread_id, &job_id).await {
            Ok(_) => app.flash = Some("stopping the job".into()),
            Err(error) => app.error(format!("cannot stop the job: {error:#}")),
        }
        app.jobs_stale = true;
    }
}
