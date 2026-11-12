//! Scheduled tasks: prompts the core sends on a timetable, each run in a new chat of its
//! project. This page lists them; `editor` is the window that makes or changes one.

pub mod editor;

use chrono::{Local, TimeZone};
use dusk_protocol::project::{Repeat, Schedule};
use gpui_kit::{
    component::{
        ActiveTheme, IconName, Sizable,
        button::{Button, ButtonVariants},
        switch::Switch,
    },
    prelude::FluentBuilder,
    *,
};

use crate::{
    pages::settings::list,
    ui::{self, Type, widgets},
    workspace::{Center, Workspace},
};

pub fn render(ws: &mut Workspace, _: &mut Window, cx: &mut Context<Workspace>) -> Stateful<Div> {
    let body = match &ws.schedules {
        None => widgets::muted("Reading the timetable...", cx).px_0(),
        Some(tasks) if tasks.is_empty() => ui::empty(
            IconName::Calendar,
            "No scheduled tasks",
            "A task sends its prompt to a new chat on its own: once, every few hours, every \
             day or on the days you choose.",
            cx,
        ),
        Some(tasks) => {
            let rows: Vec<Div> = tasks
                .clone()
                .into_iter()
                .enumerate()
                .map(|(index, task)| row(ws, index, task, cx))
                .collect();
            list(rows, cx)
        }
    };
    div()
        .id("schedules-page")
        .size_full()
        .overflow_y_scroll()
        .child(
            div()
                .max_w(px(760.))
                .px_8()
                .py_8()
                .flex()
                .flex_col()
                .gap_4()
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_start()
                        .gap_4()
                        .child(div().flex_1().child(ui::page_title(
                            "Scheduled tasks",
                            "Prompts that run on their own while duskcode is open, each in a \
                             new chat of its project.",
                            cx,
                        )))
                        .child(
                            Button::new("schedule-new")
                                .primary()
                                .icon(IconName::Plus)
                                .label("New task")
                                .on_click(cx.listener(|ws, _, window, cx| {
                                    ws.open_schedule_editor(None, window, cx)
                                })),
                        ),
                )
                .child(body),
        )
}

/// One task: what it does and when, a switch, Run now, and a button to remove it. A click
/// on its text opens it in the editor.
fn row(ws: &Workspace, index: usize, task: Schedule, cx: &mut Context<Workspace>) -> Div {
    let muted = cx.theme().muted_foreground;
    let project = ws
        .projects
        .iter()
        .find(|project| project.id == task.project_id)
        .map_or("a removed project", |project| project.name.as_str());
    let when = match task.repeat {
        Repeat::Once { at } => format!("Once, {}", moment(at)),
        ref repeat => repeat.describe(),
    };
    let next = match (task.enabled, task.next_run_at) {
        (true, Some(at)) => format!("next {}", moment(at)),
        (true, None) => "done".to_owned(),
        (false, _) => "paused".to_owned(),
    };
    let title = task.prompt.lines().next().unwrap_or_default().to_owned();
    let (id, toggled, run, delete) = (
        task.id.clone(),
        task.clone(),
        task.id.clone(),
        task.id.clone(),
    );
    let last = task.last_thread_id.clone();
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap_1()
        .min_h(px(56.))
        .px_4()
        .py_2p5()
        .child(
            div()
                .id(("schedule-open", index))
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap_0p5()
                .cursor_pointer()
                .child(div().body().truncate().child(title))
                .child(
                    div()
                        .small()
                        .text_color(muted)
                        .truncate()
                        .child(format!("{when} \u{00B7} {project} \u{00B7} {next}")),
                )
                .on_click(cx.listener(move |ws, _, window, cx| {
                    let task = ws
                        .schedules
                        .as_ref()
                        .and_then(|tasks| tasks.iter().find(|t| t.id == id).cloned());
                    ws.open_schedule_editor(task, window, cx);
                })),
        )
        .when_some(last, |row, thread_id| {
            row.child(
                Button::new(("schedule-last", index))
                    .ghost()
                    .small()
                    .icon(IconName::Inbox)
                    .tooltip("Open the chat of the last run")
                    .on_click(cx.listener(move |ws, _, _, cx| {
                        ws.center = Center::Chat;
                        ws.open_chat(&thread_id, cx);
                    })),
            )
        })
        .child(
            Button::new(("schedule-run", index))
                .ghost()
                .small()
                .icon(IconName::Play)
                .tooltip("Run it now, in a new chat")
                .on_click(cx.listener(move |ws, _, _, cx| ws.run_schedule(run.clone(), cx))),
        )
        .child(
            Button::new(("schedule-delete", index))
                .ghost()
                .small()
                .icon(IconName::Delete)
                .tooltip("Remove the task")
                .on_click(cx.listener(move |ws, _, _, cx| ws.delete_schedule(delete.clone(), cx))),
        )
        .child(
            Switch::new(("schedule-on", index))
                .checked(task.enabled)
                .on_click(cx.listener(move |ws, on: &bool, _, cx| {
                    ws.save_schedule(editor::draft_of(&toggled, *on), cx)
                })),
        )
}

/// A moment on this machine's clock: "today 09:30", "Fri 14 Nov 09:30".
pub fn moment(ms: i64) -> String {
    let Some(at) = Local.timestamp_millis_opt(ms).single() else {
        return "at an unknown time".to_owned();
    };
    if at.date_naive() == Local::now().date_naive() {
        return format!("today {}", at.format("%H:%M"));
    }
    at.format("%a %-d %b %H:%M").to_string()
}

impl Workspace {
    pub fn open_schedules(&mut self, cx: &mut Context<Self>) {
        self.center = Center::Schedules;
        self.load_schedules(cx);
    }

    pub fn load_schedules(&mut self, cx: &mut Context<Self>) {
        self.core.call(
            cx,
            |client| async move { client.projects().schedules().await },
            |ws, result, cx| ws.show_schedules(result, cx),
        );
    }

    pub fn save_schedule(
        &mut self,
        draft: dusk_protocol::project::ScheduleDraft,
        cx: &mut Context<Self>,
    ) {
        self.core.call(
            cx,
            move |client| async move { client.projects().save_schedule(draft).await },
            |ws, result, cx| {
                let saved = result.is_ok();
                ws.show_schedules(result, cx);
                if saved && matches!(ws.modal, Some(crate::modal::Modal::Schedule(_))) {
                    ws.modal = None;
                }
            },
        );
    }

    fn delete_schedule(&mut self, id: String, cx: &mut Context<Self>) {
        self.core.call(
            cx,
            move |client| async move { client.projects().delete_schedule(id).await },
            |ws, result, cx| ws.show_schedules(result, cx),
        );
    }

    fn run_schedule(&mut self, id: String, cx: &mut Context<Self>) {
        self.core.call(
            cx,
            move |client| async move { client.projects().run_schedule(id).await },
            |ws, result, cx| match result {
                Ok(_) => {
                    ws.reload_threads(cx);
                    ws.load_schedules(cx);
                }
                Err(error) => ws.fail(error, cx),
            },
        );
    }

    fn show_schedules(&mut self, result: anyhow::Result<Vec<Schedule>>, cx: &mut Context<Self>) {
        match result {
            Ok(tasks) => self.schedules = Some(tasks),
            Err(error) => self.fail(error, cx),
        }
        cx.notify();
    }
}
