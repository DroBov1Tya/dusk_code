//! The window that makes or changes a scheduled task: the project, the instructions, how
//! it repeats, and under that the choice the repeat needs: a date, an interval, a day or
//! days, and a time typed in or picked from a list.

use chrono::{Days, Local, NaiveTime, TimeZone};
use dusk_protocol::project::{
    Clock, DAY_NAMES, Repeat, Schedule, ScheduleDraft, WEEKDAYS, interval_name,
};
use gpui_kit::{
    component::{
        ActiveTheme, IconName,
        button::{Button, ButtonVariants},
        input::{Input, InputState, Textarea, TextareaState},
        switch::Switch,
    },
    prelude::FluentBuilder,
    *,
};

use crate::{
    modal::Modal,
    ui::{Type, widgets},
    workspace::Workspace,
};

/// How a task repeats, as the editor offers it.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Once,
    Interval,
    Daily,
    Weekdays,
    Weekends,
    Weekly,
    Custom,
}

const KINDS: [(Kind, &str); 7] = [
    (Kind::Once, "Once"),
    (Kind::Interval, "Interval"),
    (Kind::Daily, "Every day"),
    (Kind::Weekdays, "Weekdays"),
    (Kind::Weekends, "Weekends"),
    (Kind::Weekly, "Every week"),
    (Kind::Custom, "Custom"),
];

const INTERVALS: [u32; 9] = [15, 30, 60, 120, 180, 360, 720, 1440, 2880];

/// Which list is open over the editor.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Menu {
    Project,
    Repeat,
    Interval,
    Day,
    Date,
    Time,
    Model,
}

pub struct Editor {
    pub id: Option<String>,
    pub project_id: Option<String>,
    pub prompt: Entity<TextareaState>,
    pub kind: Kind,
    pub interval: u32,
    /// For a weekly task, 0 for Monday.
    pub day: u8,
    /// For a custom task, bit 0 for Monday.
    pub days: u8,
    /// For a single run, days from today.
    pub date: u64,
    pub time: Entity<InputState>,
    pub enabled: bool,
    /// `provider/model`; `None` runs with the chosen one.
    pub model: Option<String>,
    pub menu: Option<Menu>,
}

pub fn render(ws: &Workspace, cx: &mut Context<Workspace>) -> Div {
    let Some(Modal::Schedule(editor)) = &ws.modal else {
        return div();
    };
    let muted = cx.theme().muted_foreground;
    let label = |text: &'static str| div().small().text_color(muted).child(text);
    let project = editor
        .project_id
        .as_ref()
        .and_then(|id| ws.projects.iter().find(|project| &project.id == id))
        .map_or("Choose a project".to_owned(), |project| {
            project.name.clone()
        });
    let projects: Vec<(String, String)> = ws
        .projects
        .iter()
        .map(|project| (project.id.clone(), project.name.clone()))
        .collect();
    let kind_name = KINDS
        .iter()
        .find(|(kind, _)| *kind == editor.kind)
        .map_or("", |(_, name)| *name);
    let mut when = div()
        .flex()
        .flex_row()
        .flex_wrap()
        .items_center()
        .gap_2()
        .child(picker(
            "schedule-repeat",
            kind_name.to_owned(),
            Menu::Repeat,
            editor.menu,
            KINDS
                .iter()
                .map(|(kind, name)| {
                    let kind = *kind;
                    choice(name.to_string(), move |editor| editor.kind = kind)
                })
                .collect(),
            cx,
        ));
    match editor.kind {
        Kind::Once => {
            when = when.child(picker(
                "schedule-date",
                date_name(editor.date),
                Menu::Date,
                editor.menu,
                (0..14)
                    .map(|offset| choice(date_name(offset), move |editor| editor.date = offset))
                    .collect(),
                cx,
            ));
        }
        Kind::Interval => {
            when = when.child(picker(
                "schedule-interval",
                format!("Every {}", interval_name(editor.interval)),
                Menu::Interval,
                editor.menu,
                INTERVALS
                    .iter()
                    .map(|&minutes| {
                        choice(format!("Every {}", interval_name(minutes)), move |editor| {
                            editor.interval = minutes
                        })
                    })
                    .collect(),
                cx,
            ));
        }
        Kind::Weekly => {
            when = when.child(picker(
                "schedule-day",
                format!("on {}", DAY_NAMES[usize::from(editor.day.min(6))]),
                Menu::Day,
                editor.menu,
                (0u8..7)
                    .map(|day| {
                        choice(DAY_NAMES[usize::from(day)].to_owned(), move |editor| {
                            editor.day = day
                        })
                    })
                    .collect(),
                cx,
            ));
        }
        Kind::Custom => when = when.child(day_chips(editor.days, cx)),
        Kind::Daily | Kind::Weekdays | Kind::Weekends => {}
    }
    if editor.kind != Kind::Interval {
        when = when.child(time_field(editor, cx));
    }
    let mut models: Vec<(String, Pick)> = vec![choice("The chosen model".to_owned(), |editor| {
        editor.model = None
    })];
    for provider in &ws.providers {
        for model in provider.models.as_deref().unwrap_or_default() {
            let name = format!("{}/{}", provider.name, model.id);
            let label = name.clone();
            models.push(choice(label, move |editor| {
                editor.model = Some(name.clone())
            }));
        }
    }
    let model_label = editor
        .model
        .clone()
        .unwrap_or_else(|| "The chosen model".to_owned());
    div()
        .flex()
        .flex_col()
        .gap_2()
        .p_5()
        .child(label("Project"))
        .child(picker(
            "schedule-project",
            project,
            Menu::Project,
            editor.menu,
            projects
                .into_iter()
                .map(|(id, name)| choice(name, move |editor| editor.project_id = Some(id.clone())))
                .collect(),
            cx,
        ))
        .child(div().h_2())
        .child(label(
            "Instructions: what the agent does each time, in a new chat",
        ))
        .child(Textarea::new(&editor.prompt).h(px(160.)))
        .child(div().h_2())
        .child(label("Model"))
        .child(picker(
            "schedule-model",
            model_label,
            Menu::Model,
            editor.menu,
            models,
            cx,
        ))
        .child(div().h_2())
        .child(label("Repeat"))
        .child(when)
        .child(div().h_2())
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_3()
                .child(
                    Switch::new("schedule-enabled")
                        .checked(editor.enabled)
                        .on_click(cx.listener(|ws, on: &bool, _, cx| {
                            let on = *on;
                            ws.edit_schedule(cx, move |editor| editor.enabled = on);
                        })),
                )
                .child(div().body().child("On"))
                .child(div().flex_1())
                .child(
                    Button::new("schedule-cancel")
                        .ghost()
                        .label("Cancel")
                        .on_click(cx.listener(|ws, _, _, cx| ws.close_modal(cx))),
                )
                .child(
                    Button::new("schedule-save")
                        .primary()
                        .label("Save")
                        .on_click(cx.listener(|ws, _, _, cx| ws.submit_schedule(cx))),
                ),
        )
}

type Pick = Box<dyn Fn(&mut Editor) + 'static>;

fn choice(label: String, pick: impl Fn(&mut Editor) + 'static) -> (String, Pick) {
    (label, Box::new(pick))
}

/// A button with a caret that opens its list right under it, as the model picker does.
fn picker(
    id: &'static str,
    label: String,
    menu: Menu,
    open: Option<Menu>,
    choices: Vec<(String, Pick)>,
    cx: &mut Context<Workspace>,
) -> Div {
    let list = (open == Some(menu)).then(|| {
        let theme = cx.theme();
        let mut panel = div()
            .id(SharedString::from(format!("{id}-menu")))
            .absolute()
            .top(px(40.))
            .left_0()
            .min_w(px(200.))
            .max_h(px(280.))
            .overflow_y_scroll()
            .p_1()
            .flex()
            .flex_col()
            .rounded_xl()
            .border_1()
            .border_color(theme.border)
            .bg(theme.popover)
            .shadow_lg()
            // Painted over the window, the list must keep its clicks: the dimmed backdrop
            // under it closes the window on any press that reaches it.
            .occlude()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation());
        for (index, (label, pick)) in choices.into_iter().enumerate() {
            let pick: std::rc::Rc<dyn Fn(&mut Editor)> = pick.into();
            panel = panel.child(
                widgets::menu_item((SharedString::from(id), index), cx)
                    .child(label)
                    .on_click(cx.listener(move |ws, _, _, cx| {
                        let pick = std::rc::Rc::clone(&pick);
                        ws.edit_schedule(cx, move |editor| {
                            pick(editor);
                            editor.menu = None;
                        });
                    })),
            );
        }
        widgets::on_top(widgets::appear(
            SharedString::from(format!("{id}-fade")),
            panel,
        ))
    });
    div()
        .relative()
        .child(
            Button::new(id)
                .outline()
                .label(label)
                .dropdown_caret(true)
                .on_click(cx.listener(move |ws, _, _, cx| {
                    ws.edit_schedule(cx, move |editor| {
                        editor.menu = (editor.menu != Some(menu)).then_some(menu);
                    });
                })),
        )
        .children(list)
}

/// The time: typed as 09:30, or picked from every half hour.
fn time_field(editor: &Editor, cx: &mut Context<Workspace>) -> Div {
    let times = (0..48u8).map(|half| {
        Clock {
            hour: half / 2,
            minute: (half % 2) * 30,
        }
        .to_string()
    });
    // The list writes into the box through the workspace, which owns the window.
    let list = (editor.menu == Some(Menu::Time)).then(|| {
        let theme = cx.theme();
        let mut panel = div()
            .id("schedule-time-menu")
            .absolute()
            .top(px(40.))
            .right_0()
            .w(px(120.))
            .max_h(px(280.))
            .overflow_y_scroll()
            .p_1()
            .flex()
            .flex_col()
            .rounded_xl()
            .border_1()
            .border_color(theme.border)
            .bg(theme.popover)
            .shadow_lg()
            .occlude()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation());
        for (index, text) in times.enumerate() {
            let value = text.clone();
            panel = panel.child(
                widgets::menu_item(("schedule-time", index), cx)
                    .child(text)
                    .on_click(cx.listener(move |ws, _, window, cx| {
                        ws.pick_schedule_time(value.clone(), window, cx)
                    })),
            );
        }
        widgets::on_top(widgets::appear("schedule-time-fade", panel))
    });
    div()
        .relative()
        .flex()
        .flex_row()
        .items_center()
        .gap_1()
        .child(
            div()
                .small()
                .text_color(cx.theme().muted_foreground)
                .child("at"),
        )
        .child(div().w(px(84.)).child(Input::new(&editor.time)))
        .child(
            Button::new("schedule-time-list")
                .ghost()
                .icon(IconName::ChevronDown)
                .tooltip("Pick a time")
                .on_click(cx.listener(|ws, _, _, cx| {
                    ws.edit_schedule(cx, |editor| {
                        editor.menu = (editor.menu != Some(Menu::Time)).then_some(Menu::Time);
                    });
                })),
        )
        .children(list)
}

/// Seven toggles, Monday to Sunday, for a custom task.
fn day_chips(days: u8, cx: &mut Context<Workspace>) -> Div {
    let mut row = div().flex().flex_row().gap_1();
    for (index, name) in DAY_NAMES.iter().enumerate() {
        let bit = 1u8 << index;
        let on = days & bit != 0;
        row = row.child(
            Button::new(("schedule-chip", index))
                .label(*name)
                .map(|chip| if on { chip.primary() } else { chip.outline() })
                .on_click(cx.listener(move |ws, _, _, cx| {
                    ws.edit_schedule(cx, move |editor| editor.days ^= bit);
                })),
        );
    }
    row
}

fn date_name(offset: u64) -> String {
    match offset {
        0 => "Today".to_owned(),
        1 => "Tomorrow".to_owned(),
        _ => Local::now()
            .date_naive()
            .checked_add_days(Days::new(offset))
            .map_or("Later".to_owned(), |date| {
                date.format("%a %-d %b").to_string()
            }),
    }
}

/// A task as a draft, for a change that keeps everything but whether it is on.
pub fn draft_of(task: &Schedule, enabled: bool) -> ScheduleDraft {
    ScheduleDraft {
        id: Some(task.id.clone()),
        project_id: task.project_id.clone(),
        prompt: task.prompt.clone(),
        repeat: task.repeat.clone(),
        enabled,
        model: task.model.clone(),
    }
}

impl Workspace {
    pub fn open_schedule_editor(
        &mut self,
        task: Option<Schedule>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let prompt_text = task.as_ref().map(|t| t.prompt.clone()).unwrap_or_default();
        let prompt = cx.new(|cx| {
            let mut state = TextareaState::new(window, cx)
                .placeholder("Look through the open issues and sum up what changed today.");
            state.set_value(prompt_text, window, cx);
            state
        });
        let mut editor = Editor {
            id: None,
            project_id: self
                .selected_project()
                .or(self.projects.first())
                .map(|project| project.id.clone()),
            prompt,
            kind: Kind::Daily,
            interval: 60,
            day: 0,
            days: WEEKDAYS,
            date: 0,
            time: cx.new(|cx| InputState::new(window, cx).placeholder("09:00")),
            enabled: true,
            model: None,
            menu: None,
        };
        if self.providers.is_empty() {
            self.load_models(cx);
        }
        let mut time = Clock { hour: 9, minute: 0 };
        if let Some(task) = task {
            editor.id = Some(task.id);
            editor.project_id = Some(task.project_id);
            editor.enabled = task.enabled;
            editor.model = task.model.clone();
            match task.repeat {
                Repeat::Once { at } => {
                    editor.kind = Kind::Once;
                    if let Some(moment) = Local.timestamp_millis_opt(at).single() {
                        let days = (moment.date_naive() - Local::now().date_naive()).num_days();
                        editor.date = u64::try_from(days).unwrap_or(0);
                        time = clock_of(moment.time());
                    }
                }
                Repeat::Interval { minutes } => {
                    editor.kind = Kind::Interval;
                    editor.interval = minutes;
                }
                Repeat::Daily { at } => (editor.kind, time) = (Kind::Daily, at),
                Repeat::Weekdays { at } => (editor.kind, time) = (Kind::Weekdays, at),
                Repeat::Weekends { at } => (editor.kind, time) = (Kind::Weekends, at),
                Repeat::Weekly { day, at } => {
                    (editor.kind, editor.day, time) = (Kind::Weekly, day, at);
                }
                Repeat::Custom { days, at } => {
                    (editor.kind, editor.days, time) = (Kind::Custom, days, at);
                }
            }
        }
        editor.time.update(cx, |state, cx| {
            state.set_value(time.to_string(), window, cx)
        });
        self.modal = Some(Modal::Schedule(editor));
        cx.notify();
    }

    fn edit_schedule(&mut self, cx: &mut Context<Self>, edit: impl FnOnce(&mut Editor)) {
        if let Some(Modal::Schedule(editor)) = &mut self.modal {
            edit(editor);
        }
        cx.notify();
    }

    fn pick_schedule_time(&mut self, text: String, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(Modal::Schedule(editor)) = &mut self.modal {
            editor.menu = None;
            editor
                .time
                .update(cx, |state, cx| state.set_value(text, window, cx));
        }
        cx.notify();
    }

    /// Reads the editor into a draft and asks the core to keep it; the core checks it
    /// again and works out when it runs.
    fn submit_schedule(&mut self, cx: &mut Context<Self>) {
        let Some(Modal::Schedule(editor)) = &self.modal else {
            return;
        };
        let draft = match draft(editor, cx) {
            Ok(draft) => draft,
            Err(error) => {
                self.fail(error, cx);
                return;
            }
        };
        self.save_schedule(draft, cx);
    }
}

fn draft(editor: &Editor, cx: &App) -> anyhow::Result<ScheduleDraft> {
    let project_id = editor
        .project_id
        .clone()
        .ok_or_else(|| anyhow::anyhow!("choose a project for the task"))?;
    let typed = editor.time.read(cx).value().to_string();
    let at = || Clock::parse(&typed).ok_or_else(|| anyhow::anyhow!("write the time as 09:30"));
    let repeat = match editor.kind {
        Kind::Once => {
            let at = at()?;
            let date = Local::now()
                .date_naive()
                .checked_add_days(Days::new(editor.date))
                .ok_or_else(|| anyhow::anyhow!("the date is out of range"))?;
            let time = NaiveTime::from_hms_opt(u32::from(at.hour), u32::from(at.minute), 0)
                .ok_or_else(|| anyhow::anyhow!("write the time as 09:30"))?;
            let moment = Local
                .from_local_datetime(&date.and_time(time))
                .earliest()
                .ok_or_else(|| anyhow::anyhow!("that time does not exist on this day"))?;
            Repeat::Once {
                at: moment.timestamp_millis(),
            }
        }
        Kind::Interval => Repeat::Interval {
            minutes: editor.interval,
        },
        Kind::Daily => Repeat::Daily { at: at()? },
        Kind::Weekdays => Repeat::Weekdays { at: at()? },
        Kind::Weekends => Repeat::Weekends { at: at()? },
        Kind::Weekly => Repeat::Weekly {
            day: editor.day,
            at: at()?,
        },
        Kind::Custom => Repeat::Custom {
            days: editor.days,
            at: at()?,
        },
    };
    Ok(ScheduleDraft {
        id: editor.id.clone(),
        project_id,
        prompt: editor.prompt.read(cx).value().to_string(),
        repeat,
        enabled: editor.enabled,
        model: editor.model.clone(),
    })
}

fn clock_of(time: NaiveTime) -> Clock {
    use chrono::Timelike;
    Clock {
        hour: u8::try_from(time.hour()).unwrap_or(0),
        minute: u8::try_from(time.minute()).unwrap_or(0),
    }
}
