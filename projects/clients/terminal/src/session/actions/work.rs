//! Actions on what a thread works on: its plan, changes, sources and jobs.

use dusk_core::client::Client;
use dusk_protocol::client::Command;

use crate::app::{App, DiffView, Panel, PasteEditor};
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
