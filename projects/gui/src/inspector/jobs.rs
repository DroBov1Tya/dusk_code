//! The shown chat's background jobs, with a stop for those this run owns.

use gpui_kit::{
    component::{
        ActiveTheme, Sizable,
        button::{Button, ButtonVariants},
    },
    *,
};

use crate::{widgets, workspace::Workspace};

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
                .xsmall()
                .label("Stop")
                .on_click(cx.listener(move |ws, _, _, cx| ws.stop_job(&id, cx)))
        });
        column = column.child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .p_2()
                .rounded_md()
                .bg(theme.muted)
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

impl Workspace {
    pub fn load_jobs(&mut self, cx: &mut Context<Self>) {
        let Some(thread_id) = self.shown.clone() else {
            return;
        };
        self.core.call(
            cx,
            move |client| async move { client.work().jobs(&thread_id).await },
            |ws, result, cx| match result {
                Ok(jobs) => {
                    ws.jobs = jobs;
                    cx.notify();
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
