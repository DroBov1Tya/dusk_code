//! Scheduled tasks, as the desktop app has them: the list with Run now, a switch and
//! Remove, and a form that makes or changes one.

use crate::{
    api,
    state::{View, state},
    ui::{run, when},
};
use dusk_protocol::project::{
    Clock, DAY_NAMES, Repeat, Schedule, ScheduleDraft, WEEKDAYS, interval_name,
};
use leptos::prelude::*;

const INTERVALS: [u32; 9] = [15, 30, 60, 120, 180, 360, 720, 1440, 2880];

/// The form's fields, as plain text the inputs hold.
#[derive(Clone)]
struct Form {
    id: Option<String>,
    project_id: String,
    prompt: String,
    kind: String,
    interval: u32,
    day: u8,
    days: u8,
    date: String,
    time: String,
    enabled: bool,
}

impl Form {
    fn new(project_id: String) -> Self {
        Self {
            id: None,
            project_id,
            prompt: String::new(),
            kind: "daily".into(),
            interval: 60,
            day: 0,
            days: WEEKDAYS,
            date: String::new(),
            time: "09:00".into(),
            enabled: true,
        }
    }

    fn of(task: &Schedule) -> Self {
        let mut form = Self::new(task.project_id.clone());
        form.id = Some(task.id.clone());
        form.prompt = task.prompt.clone();
        form.enabled = task.enabled;
        let mut set = |kind: &str, at: Clock| {
            form.kind = kind.into();
            form.time = at.to_string();
        };
        match task.repeat {
            Repeat::Once { .. } => form.kind = "once".into(),
            Repeat::Interval { minutes } => {
                form.kind = "interval".into();
                form.interval = minutes;
            }
            Repeat::Daily { at } => set("daily", at),
            Repeat::Weekdays { at } => set("weekdays", at),
            Repeat::Weekends { at } => set("weekends", at),
            Repeat::Weekly { day, at } => {
                set("weekly", at);
                form.day = day;
            }
            Repeat::Custom { days, at } => {
                set("custom", at);
                form.days = days;
            }
        }
        form
    }

    fn draft(&self) -> Result<ScheduleDraft, String> {
        let at = || Clock::parse(&self.time).ok_or("write the time as 09:30".to_owned());
        let repeat = match self.kind.as_str() {
            "once" => Repeat::Once {
                at: moment(&self.date, at()?)?,
            },
            "interval" => Repeat::Interval {
                minutes: self.interval,
            },
            "weekdays" => Repeat::Weekdays { at: at()? },
            "weekends" => Repeat::Weekends { at: at()? },
            "weekly" => Repeat::Weekly {
                day: self.day,
                at: at()?,
            },
            "custom" => Repeat::Custom {
                days: self.days,
                at: at()?,
            },
            _ => Repeat::Daily { at: at()? },
        };
        Ok(ScheduleDraft {
            id: self.id.clone(),
            project_id: self.project_id.clone(),
            prompt: self.prompt.clone(),
            repeat,
            enabled: self.enabled,
        })
    }
}

/// A date from a date input and a time, on the browser's clock, in Unix milliseconds.
fn moment(date: &str, at: Clock) -> Result<i64, String> {
    let mut parts = date.split('-').map(|part| part.parse::<i32>().ok());
    let (Some(Some(year)), Some(Some(month)), Some(Some(day))) =
        (parts.next(), parts.next(), parts.next())
    else {
        return Err("choose a date".into());
    };
    let date = js_sys::Date::new_with_year_month_day_hr_min(
        u32::try_from(year).map_err(|_| "choose a date".to_owned())?,
        month - 1,
        day,
        i32::from(at.hour),
        i32::from(at.minute),
    );
    Ok(date.get_time() as i64)
}

#[component]
pub fn Schedules() -> impl IntoView {
    let app = state();
    let tasks = RwSignal::new(None::<Vec<Schedule>>);
    let form = RwSignal::new(None::<Form>);
    let shown = move |list: Vec<Schedule>| tasks.set(Some(list));
    run(api::schedules(), shown);
    let save = move |draft: ScheduleDraft, close: bool| {
        run(api::save_schedule(draft), move |list| {
            shown(list);
            if close {
                form.set(None);
            }
        })
    };
    let project_name = move |id: &str| {
        app.projects
            .with(|projects| projects.iter().find(|p| p.id == id).map(|p| p.name.clone()))
            .unwrap_or_else(|| "a removed project".into())
    };
    view! {
        <div class="page">
            <div class="page-head">
                <div>
                    <h2>"Scheduled tasks"</h2>
                    <p class="muted">"Prompts that run on their own while duskcode is open, each in a new chat of its project."</p>
                </div>
                <button class="primary" on:click=move |_| {
                    let first = app.projects.with(|p| p.first().map(|p| p.id.clone())).unwrap_or_default();
                    form.set(Some(Form::new(first)));
                }>"New task"</button>
            </div>
            {move || form.get().map(|_| view! { <Editor form save /> })}
            {move || match tasks.get() {
                None => view! { <p class="muted">"Reading the timetable..."</p> }.into_any(),
                Some(list) if list.is_empty() => view! {
                    <p class="muted">"No scheduled tasks. A task sends its prompt to a new chat on its own: once, every few hours, every day or on the days you choose."</p>
                }.into_any(),
                Some(list) => view! {
                    <div class="section">
                        {list.into_iter().map(|task| {
                            let when_text = match task.repeat {
                                Repeat::Once { at } => format!("Once, {}", when(at / 1000)),
                                ref repeat => repeat.describe(),
                            };
                            let next = match (task.enabled, task.next_run_at) {
                                (true, Some(at)) => format!("next {}", when(at / 1000)),
                                (true, None) => "done".into(),
                                (false, _) => "paused".into(),
                            };
                            let hint = format!("{when_text} · {} · {next}", project_name(&task.project_id));
                            let title = task.prompt.lines().next().unwrap_or_default().to_owned();
                            let (edit, toggle, run_id, delete_id) =
                                (task.clone(), task.clone(), task.id.clone(), task.id.clone());
                            let last = task.last_thread_id.clone();
                            view! {
                                <div class="row">
                                    <div class="row-text grow clickable" on:click=move |_| form.set(Some(Form::of(&edit)))>
                                        <div class="tile-name">{title}</div>
                                        <div class="row-hint">{hint}</div>
                                    </div>
                                    <div class="row-control">
                                        {last.map(|thread_id| view! {
                                            <button class="ghost" title="Open the chat of the last run"
                                                on:click=move |_| app.view.set(View::Chat(Some(thread_id.clone())))>"Last run"</button>
                                        })}
                                        <button class="ghost" title="Run it now, in a new chat" on:click=move |_| {
                                            run(api::run_schedule(run_id.clone()), move |_| run(api::schedules(), shown));
                                        }>"Run now"</button>
                                        <button class="ghost danger" on:click=move |_| run(api::delete_schedule(delete_id.clone()), shown)>"Remove"</button>
                                        <label class="switch" title="On or paused">
                                            <input type="checkbox" prop:checked=task.enabled on:change=move |ev| {
                                                let mut draft = Form::of(&toggle);
                                                draft.enabled = event_target_checked(&ev);
                                                match draft.draft() {
                                                    Ok(draft) => save(draft, false),
                                                    Err(error) => app.fail(error),
                                                }
                                            } />
                                            <span class="track"></span>
                                        </label>
                                    </div>
                                </div>
                            }
                        }).collect_view()}
                    </div>
                }.into_any(),
            }}
        </div>
    }
}

/// The form over the list: project, instructions, how it repeats and when.
#[component]
fn Editor(
    form: RwSignal<Option<Form>>,
    save: impl Fn(ScheduleDraft, bool) + Copy + 'static,
) -> impl IntoView {
    let app = state();
    let field = move |edit: fn(&mut Form, String)| {
        move |ev: leptos::ev::Event| {
            let value = event_target_value(&ev);
            form.update(|form| {
                if let Some(form) = form {
                    edit(form, value);
                }
            });
        }
    };
    let get = move |read: fn(&Form) -> String| form.with(|f| f.as_ref().map(read).unwrap_or_default());
    let kind = move || get(|f| f.kind.clone());
    view! {
        <div class="card-tile editor">
            <h3>{move || if form.with(|f| f.as_ref().is_some_and(|f| f.id.is_some())) { "Edit task" } else { "New task" }}</h3>
            <label>"Project"
                <select prop:value=move || get(|f| f.project_id.clone()) on:change=field(|f, v| f.project_id = v)>
                    {move || app.projects.get().into_iter().map(|p| view! { <option value=p.id.clone()>{p.name}</option> }).collect_view()}
                </select>
            </label>
            <label>"Instructions: what the agent does each time, in a new chat"
                <textarea rows="6" prop:value=move || get(|f| f.prompt.clone()) on:input=field(|f, v| f.prompt = v)
                    placeholder="Look through the open issues and sum up what changed today."></textarea>
            </label>
            <div class="row-control left">
                <select prop:value=kind on:change=field(|f, v| f.kind = v)>
                    <option value="once">"Once"</option>
                    <option value="interval">"Interval"</option>
                    <option value="daily">"Every day"</option>
                    <option value="weekdays">"Weekdays"</option>
                    <option value="weekends">"Weekends"</option>
                    <option value="weekly">"Every week"</option>
                    <option value="custom">"Custom"</option>
                </select>
                {move || match kind().as_str() {
                    "once" => view! {
                        <input type="date" prop:value=move || get(|f| f.date.clone()) on:change=field(|f, v| f.date = v) />
                    }.into_any(),
                    "interval" => view! {
                        <select prop:value=move || get(|f| f.interval.to_string()) on:change=field(|f, v| f.interval = v.parse().unwrap_or(60))>
                            {INTERVALS.iter().map(|m| view! { <option value=m.to_string()>{format!("Every {}", interval_name(*m))}</option> }).collect_view()}
                        </select>
                    }.into_any(),
                    "weekly" => view! {
                        <select prop:value=move || get(|f| f.day.to_string()) on:change=field(|f, v| f.day = v.parse().unwrap_or(0))>
                            {DAY_NAMES.iter().enumerate().map(|(i, n)| view! { <option value=i.to_string()>{format!("on {n}")}</option> }).collect_view()}
                        </select>
                    }.into_any(),
                    "custom" => view! {
                        <span class="days">
                            {DAY_NAMES.iter().enumerate().map(|(i, n)| {
                                let bit = 1u8 << i;
                                view! {
                                    <button class=move || if form.with(|f| f.as_ref().is_some_and(|f| f.days & bit != 0)) { "primary" } else { "ghost" }
                                        on:click=move |_| form.update(|f| if let Some(f) = f { f.days ^= bit })>{*n}</button>
                                }
                            }).collect_view()}
                        </span>
                    }.into_any(),
                    _ => ().into_any(),
                }}
                {move || (kind() != "interval").then(|| view! {
                    <span class="muted">"at"</span>
                    <input type="time" prop:value=move || get(|f| f.time.clone()) on:change=field(|f, v| f.time = v) />
                })}
            </div>
            <div class="row-control">
                <button class="ghost" on:click=move |_| form.set(None)>"Cancel"</button>
                <button class="primary" on:click=move |_| {
                    match form.with(|f| f.as_ref().map(Form::draft)) {
                        Some(Ok(draft)) => save(draft, true),
                        Some(Err(error)) => app.fail(error),
                        None => {}
                    }
                }>"Save"</button>
            </div>
        </div>
    }
}
