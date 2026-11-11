//! What the core answered to a command that does not run in the thread, and the
//! panels whose numbers come from the core.

use std::time::Instant;

use dusk_core::client::Client;
use dusk_protocol::{
    command::{Answer, Command},
    model::Effort,
};

use super::{
    actions::set_update_note,
    run::{JOBS_PERIOD, SYSTEM_PERIOD},
    thread::{OpenThread, close},
};
use crate::app::{
    App, EFFORTS, JobsView, ModelList, Panel, Preview, SettingCategory, SettingsView, ThreadList,
    Tone,
};

/// Shows what the core answered to a command that does not run in the thread.
pub async fn answer(
    client: &Client,
    app: &mut App,
    open: &mut Option<OpenThread>,
    command: Command,
) {
    let migrating = matches!(&command, Command::Migrate { .. });
    match client.command(command).await {
        Ok(Answer::Migrated(path)) => {
            app.panel = None;
            app.flash = None;
            app.transcript.notice(format!(
                "Archive saved: {}\nContains chat history; keep it private. Keys kept in the system keychain are not in it: enter them again after a restore.\nRestore offline with duskcode restore <archive> into empty data roots. Saved absolute project paths are unchanged.",
                path.display()
            ), Tone::Info);
        }
        Ok(Answer::Threads(threads)) => {
            let current = app.thread_id.as_deref();
            let selected = threads
                .iter()
                .position(|thread| Some(thread.id.as_str()) == current)
                .unwrap_or(0);
            let first = threads.get(selected).map(|thread| thread.id.clone());
            app.panel = Some(Panel::Threads(ThreadList {
                threads,
                selected,
                preview: None,
            }));
            if let Some(thread_id) = first {
                preview(client, app, thread_id).await;
            }
        }
        Ok(Answer::Providers { current, providers }) => {
            app.panel = Some(Panel::Models(ModelList::checked(current, providers)));
        }
        Ok(Answer::ModelSwitched { name }) => {
            app.panel = None;
            app.limit = client.status.borrow().input_limit;
            app.flash = Some(format!("model {name}"));
            app.model = name;
            app.provider_available = true;
        }
        Ok(Answer::Settings(settings)) => match &mut app.panel {
            Some(Panel::Settings(view)) => view.refresh(*settings),
            _ => app.panel = Some(Panel::Settings(Box::new(SettingsView::new(*settings)))),
        },
        Ok(Answer::Effort { current, changed }) => {
            app.effort = current;
            let name = current.map_or("default", Effort::name);
            if changed {
                app.flash = Some(format!("effort {name}"));
            } else {
                let selected = EFFORTS.iter().position(|effort| *effort == name);
                app.panel = Some(Panel::Effort {
                    selected: selected.unwrap_or(0),
                });
            }
        }
        Ok(Answer::UndoPoints(points)) if points.is_empty() => {
            app.flash = Some("nothing to undo yet".into());
        }
        Ok(Answer::UndoPoints(points)) => {
            let selected = points.len() - 1;
            app.panel = Some(Panel::Undo { points, selected });
        }
        Ok(Answer::Sources(found)) if found.is_empty() => {
            app.panel = None;
            app.flash = Some("no sources; /sources <path> adds a file".into());
        }
        Ok(Answer::Sources(found)) => {
            let selected = match &app.panel {
                Some(Panel::Sources { selected, .. }) => (*selected).min(found.len() - 1),
                _ => found.len() - 1,
            };
            app.panel = Some(Panel::Sources {
                sources: found,
                selected,
                removing: false,
            });
        }
        Ok(Answer::Jobs(jobs)) if !jobs.is_empty() => {
            app.panel = Some(Panel::Jobs(JobsView {
                selected: jobs.len() - 1,
                jobs,
                output: String::new(),
                stale: true,
            }));
        }
        Ok(Answer::Jobs(_)) => app.flash = Some("no background jobs in this thread".into()),
        Ok(Answer::Update(info)) => {
            app.panel = Some(Panel::Update {
                info: Some(info),
                note: None,
                progress: None,
                button: None,
            });
        }
        Ok(Answer::Context(report)) => {
            app.panel = Some(Panel::Context { report, row: 0 });
        }
        Ok(Answer::NewThread) => {
            close(client, open.take()).await;
            app.panel = None;
            app.open(None, None, &[]);
        }
        // The update window stays and says the whole reason.
        Err(error) if matches!(app.panel, Some(Panel::Update { .. })) => {
            set_update_note(app, format!("cannot check for updates: {error:#}"));
        }
        Err(error) => {
            if migrating {
                app.flash = None;
            }
            app.panel = None;
            app.error(format!("{error:#}"));
        }
    }
}

/// The jobs window is open, or a job runs: the clock reads them every `JOBS_PERIOD`.
pub fn jobs_wanted(app: &App) -> bool {
    matches!(app.panel, Some(Panel::Jobs(_))) || app.jobs.iter().any(|job| job.running)
}

/// Reads the open thread's jobs, and the output of the one chosen in the window, when
/// something changed, and every `JOBS_PERIOD` while one runs or the window is open.
pub async fn refresh_jobs(client: &Client, app: &mut App) {
    let Some(thread_id) = app.thread_id.clone() else {
        app.jobs.clear();
        return;
    };
    let in_window = matches!(app.panel, Some(Panel::Jobs(_)));
    let window_stale = matches!(&app.panel, Some(Panel::Jobs(view)) if view.stale);
    let due = app.jobs_read.elapsed() >= JOBS_PERIOD;
    let running = app.jobs.iter().any(|job| job.running);
    if !(app.jobs_stale || window_stale || (due && (in_window || running))) {
        return;
    }
    app.jobs_stale = false;
    app.jobs_read = Instant::now();
    match client.work().jobs(&thread_id).await {
        Ok(jobs) => app.jobs = jobs,
        Err(error) => return app.error(format!("cannot read the jobs: {error:#}")),
    }
    if let Some(Panel::Jobs(view)) = &mut app.panel {
        view.jobs = app.jobs.clone();
        view.selected = view.selected.min(view.jobs.len().saturating_sub(1));
        view.stale = false;
        view.output = match view.jobs.get(view.selected) {
            Some(job) => client
                .work()
                .job_output(&thread_id, &job.job_id)
                .await
                .unwrap_or_else(|error| format!("cannot read the output: {error:#}")),
            None => String::new(),
        };
    }
}

pub fn system_open(app: &App) -> bool {
    matches!(&app.panel, Some(Panel::Settings(view))
        if view.category == Some(SettingCategory::System))
}

/// The system tab reads its numbers when opened and then every `SYSTEM_PERIOD`.
pub async fn refresh_system(client: &Client, app: &mut App) {
    let Some(Panel::Settings(view)) = &mut app.panel else {
        return;
    };
    let due = view
        .system
        .as_ref()
        .is_none_or(|(_, read)| read.elapsed() >= SYSTEM_PERIOD);
    if view.category != Some(SettingCategory::System) || !due {
        return;
    }
    match client.system_report().await {
        Ok(report) => view.system = Some((report, Instant::now())),
        Err(error) => app.error(format!("cannot read the system numbers: {error:#}")),
    }
}

/// Shows a thread in the threads panel, if the panel is still open. A preview that
/// cannot be read just stays empty; opening the thread reports the error.
pub async fn preview(client: &Client, app: &mut App, thread_id: String) {
    let Ok(history) = client.threads().messages(&thread_id).await else {
        return;
    };
    if let Some(Panel::Threads(list)) = &mut app.panel {
        list.preview = Some(Preview::new(thread_id, &history));
    }
}
