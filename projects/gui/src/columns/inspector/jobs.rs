//! The shown chat's background jobs, with a stop for those this run owns.

use gpui_kit::{
    component::{
        ActiveTheme, Sizable,
        button::{Button, ButtonVariants},
    },
    *,
};

use crate::{ui::widgets, workspace::Workspace};

pub fn render(ws: &Workspace, cx: &mut Context<Workspace>) -> Div {
    if ws.jobs.is_empty() {
        return div().child(widgets::muted("No background jobs in this chat.", cx));
    }
    let theme = cx.theme();
    let (accent, muted) = (theme.primary, theme.muted_foreground);
    let mut column = div().flex().flex_col().gap_1().p_2();
    for job in &ws.jobs {
        let id = job.job_id.clone();
        let stop = (job.running && job.stoppable).then(|| {
            Button::new(SharedString::from(format!("stop-{id}")))
                .danger()
                .small()
                .label("Stop")
                .on_click(cx.listener(move |ws, _, _, cx| {
                    cx.stop_propagation();
                    ws.stop_job(&id, cx);
                }))
        });
        let (open, hover) = (job.job_id.clone(), theme.list_hover);
        column = column.child(
            div()
                .id(SharedString::from(format!("job-{}", job.job_id)))
                .flex()
                .flex_col()
                .gap_1()
                .p_2()
                .rounded_md()
                .bg(theme.muted)
                .cursor_pointer()
                .hover(move |card| card.bg(hover))
                .on_click(cx.listener(move |ws, _, _, cx| ws.open_job(&open, cx)))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_2()
                        .child(widgets::badge(
                            job.status.clone(),
                            if job.running { accent } else { muted },
                        ))
                        .child(div().flex_1())
                        .child(div().text_xs().text_color(muted).child(ran(job)))
                        .children(stop),
                )
                .child(
                    div()
                        .text_xs()
                        .font_family("Menlo")
                        .line_clamp(2)
                        .child(job.command.clone()),
                ),
        );
    }
    column
}

/// How long a job runs, or ran.
fn ran(job: &dusk_protocol::thread::JobInfo) -> String {
    let end = job
        .finished_at
        .unwrap_or_else(crate::pages::chat::timing::now_ms);
    dusk_protocol::thread::duration_words((end - job.started_at).max(0))
}

/// One job, whole: a card in the side panel has room for two lines of its command.
pub fn detail(
    ws: &Workspace,
    job_id: &str,
    output: Option<&str>,
    cx: &mut Context<Workspace>,
) -> Div {
    let Some(job) = ws.jobs.iter().find(|job| job.job_id == job_id) else {
        return div()
            .p_5()
            .child(widgets::muted("This job is not in the chat any more.", cx));
    };
    let theme = cx.theme();
    let (accent, muted, surface) = (theme.primary, theme.muted_foreground, theme.muted);
    let mono = crate::ui::theme::prefs(cx)
        .mono_font
        .clone()
        .unwrap_or_else(|| "Menlo".to_owned());
    let block = |name: &'static str, title: &'static str, text: &str| {
        let group = SharedString::from(format!("job-{name}"));
        div()
            .group(group.clone())
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .child(div().text_xs().text_color(muted).child(title))
                    .child(widgets::copy(
                        SharedString::from(format!("job-copy-{name}")),
                        &group,
                        text,
                    )),
            )
            .child(
                div()
                    .p_3()
                    .rounded_md()
                    .bg(surface)
                    .text_xs()
                    .font_family(mono.clone())
                    .child(text.to_owned()),
            )
    };
    let id = job.job_id.clone();
    let cancel = (job.running && job.stoppable).then(|| {
        Button::new("job-cancel")
            .danger()
            .small()
            .label("Cancel")
            .tooltip("Stops the command; the agent is told it was cancelled")
            .on_click(cx.listener(move |ws, _, _, cx| ws.stop_job(&id, cx)))
    });
    let foreign = (job.running && !job.stoppable).then(|| {
        div()
            .text_xs()
            .text_color(muted)
            .child("Another duskcode runs it, so it is stopped from there.")
    });
    let mut column = div()
        .flex()
        .flex_col()
        .gap_4()
        .p_5()
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_3()
                .child(widgets::badge(
                    job.status.clone(),
                    if job.running { accent } else { muted },
                ))
                .child(div().text_sm().child(if job.running {
                    format!("Running for {}", ran(job))
                } else {
                    format!("Ran for {}", ran(job))
                }))
                .child(div().flex_1())
                .children(cancel),
        )
        .children(foreign)
        .child(block("command", "Command", &job.command));
    column = match output {
        Some(output) if !output.trim().is_empty() => {
            column.child(block("output", "Output so far", output.trim_end()))
        }
        Some(_) => column.child(widgets::muted("It printed nothing yet.", cx)),
        None => column.child(widgets::muted("Reading its output...", cx)),
    };
    column
}

impl Workspace {
    /// Opens a job's window and keeps it current while it is open: the clock every
    /// second, the job and its output with it.
    pub fn open_job(&mut self, job_id: &str, cx: &mut Context<Self>) {
        self.modal = Some(crate::modal::Modal::Job {
            job_id: job_id.to_owned(),
            output: None,
        });
        cx.notify();
        let watched = job_id.to_owned();
        cx.spawn(async move |this, cx| {
            loop {
                let open = this.update(cx, |ws, cx| {
                    let open = matches!(&ws.modal, Some(crate::modal::Modal::Job { job_id, .. })
                        if *job_id == watched);
                    if open {
                        ws.read_job(&watched, cx);
                    }
                    open
                });
                if !matches!(open, Ok(true)) {
                    return;
                }
                cx.background_executor()
                    .timer(std::time::Duration::from_secs(1))
                    .await;
            }
        })
        .detach();
    }

    fn read_job(&mut self, job_id: &str, cx: &mut Context<Self>) {
        let Some(thread_id) = self.shown.clone() else {
            return;
        };
        // A job that ended is read once more for its last lines, then left alone.
        let running = self
            .jobs
            .iter()
            .any(|job| job.job_id == job_id && job.running);
        let read = matches!(&self.modal, Some(crate::modal::Modal::Job { output, .. })
            if output.is_some());
        if read && !running {
            return;
        }
        let asked = job_id.to_owned();
        self.core.call(
            cx,
            move |client| async move {
                let work = client.work();
                let output = work.job_output(&thread_id, &asked).await?;
                let jobs = work.jobs(&thread_id).await?;
                anyhow::Ok((thread_id, asked, output, jobs))
            },
            |ws, result, cx| {
                let Ok((thread_id, asked, read, jobs)) = result else {
                    return;
                };
                if ws.shown.as_deref() != Some(thread_id.as_str()) {
                    return;
                }
                ws.jobs = jobs;
                if let Some(crate::modal::Modal::Job { job_id, output }) = &mut ws.modal
                    && *job_id == asked
                {
                    *output = Some(read);
                }
                cx.notify();
            },
        );
    }

    pub fn load_jobs(&mut self, cx: &mut Context<Self>) {
        let Some(thread_id) = self.shown.clone() else {
            return;
        };
        self.core.call(
            cx,
            move |client| async move {
                let jobs = client.work().jobs(&thread_id).await?;
                anyhow::Ok((thread_id, jobs))
            },
            |ws, result, cx| match result {
                // An answer for a chat left meanwhile is not this chat's.
                Ok((thread_id, jobs)) => {
                    if ws.shown.as_deref() == Some(thread_id.as_str()) {
                        ws.jobs = jobs;
                        cx.notify();
                    }
                }
                Err(error) => ws.fail(error, cx),
            },
        );
    }

    fn stop_job(&mut self, job_id: &str, cx: &mut Context<Self>) {
        let Some(thread_id) = self.shown.clone() else {
            return;
        };
        let job_id = job_id.to_owned();
        self.core.call(
            cx,
            move |client| async move { client.work().stop_job(&thread_id, &job_id).await },
            |ws, result, cx| match result {
                Ok(_) => ws.load_jobs(cx),
                Err(error) => ws.fail(error, cx),
            },
        );
    }
}
