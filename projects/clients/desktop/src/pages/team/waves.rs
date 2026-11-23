//! The plan in waves, as the chat's plan card and the right column's Team screen both
//! draw it: a row a wave (its name, done of all, a small square a task), unfolding to its
//! tasks, and a task to who holds it, its last note and what can be done about it. The
//! waves are the protocol's (`team_layout::waves`); only the folds are the window's.

use std::{collections::HashMap, time::Instant};

use dusk_protocol::thread::{
    GoalCheck, RunMark, Subagent, Task, TaskStatus, duration_words, role_name, short_model,
    team_layout,
};
use gpui_kit::{
    component::{
        ActiveTheme, IconName, Sizable,
        button::{Button, ButtonVariants},
    },
    prelude::FluentBuilder,
    *,
};

use crate::{
    pages::chat::activity,
    ui::{
        Type, avatar,
        looping::Loop,
        widgets::{self, Focusable},
    },
    workspace::Workspace,
};

/// A task's square.
const SQUARE: f32 = 9.;
/// How long a square of a task that just came takes to fade in.
pub const ARRIVE: std::time::Duration = std::time::Duration::from_millis(220);
/// One breath of what runs, as the mockup's.
const BREATH: std::time::Duration = std::time::Duration::from_millis(1800);

/// What is unfolded, for one chat; another chat's folds read as the defaults.
#[derive(Default)]
pub struct Folds {
    chat: Option<String>,
    /// Waves the user opened or closed, by index; the rest follow the default.
    waves: HashMap<usize, bool>,
    /// The task whose details show.
    task: Option<String>,
}

/// Where a view keeps its folds, so one widget serves two places.
pub type FoldsOf = fn(&mut Workspace) -> &mut Folds;

impl Folds {
    fn of(&self, chat: &str) -> Option<&Self> {
        (self.chat.as_deref() == Some(chat)).then_some(self)
    }

    fn mine(&mut self, chat: &str) -> &mut Self {
        if self.chat.as_deref() != Some(chat) {
            *self = Self {
                chat: Some(chat.to_owned()),
                ..Self::default()
            };
        }
        self
    }

    /// A wave is open when the user opened it, or when it is the one in work.
    fn wave_open(&self, chat: &str, wave: usize, current: Option<usize>) -> bool {
        let chosen = self.of(chat).and_then(|folds| folds.waves.get(&wave));
        chosen.copied().unwrap_or(Some(wave) == current)
    }

    fn task_open(&self, chat: &str, task_id: &str) -> bool {
        self.of(chat)
            .is_some_and(|folds| folds.task.as_deref() == Some(task_id))
    }

    /// Unfolds `task` and its wave, as a click on what needs the user does.
    pub fn show_task(&mut self, chat: &str, wave: usize, task_id: &str) {
        let folds = self.mine(chat);
        folds.waves.insert(wave, true);
        folds.task = Some(task_id.to_owned());
    }
}

/// The wave in work: the first that holds a task not done.
fn current(tasks: &[Task], waves: &[Vec<usize>]) -> Option<usize> {
    waves
        .iter()
        .position(|wave| wave.iter().any(|&at| tasks[at].status != TaskStatus::Done))
}

/// Its review waits for the user's confirmation.
pub fn for_user(task: &Task) -> bool {
    task.status == TaskStatus::Review && task.gate == Some(GoalCheck::UserConfirmation)
}

/// "2 running · 2 done · 1 needs you", the last part brighter, behind the accent's dot.
pub fn summary(tasks: &[Task], cx: &App) -> Div {
    let theme = cx.theme();
    let running = tasks.iter().filter(|task| task.status.at_work()).count();
    let done = tasks
        .iter()
        .filter(|task| task.status == TaskStatus::Done)
        .count();
    let need = tasks.iter().filter(|task| for_user(task)).count();
    // Each part whole, the dot that parts them leading the next: a narrow card wraps the
    // line between parts rather than cutting one.
    let mut parts = Vec::new();
    if running > 0 {
        parts.push(div().child(format!("{running} running")));
    }
    parts.push(div().child(format!("{done} done")));
    if need > 0 {
        parts.push(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_1p5()
                .text_color(theme.foreground.opacity(0.8))
                .child(div().size(px(6.)).rounded_full().bg(theme.primary))
                .child(format!("{need} needs you")),
        );
    }
    let mut line = div()
        .flex()
        .flex_row()
        .flex_wrap()
        .items_center()
        .gap_x_1()
        .small()
        .text_color(theme.muted_foreground);
    for (at, part) in parts.into_iter().enumerate() {
        line = line.child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_1()
                .when(at > 0, |part| part.child("\u{00B7}"))
                .child(part),
        );
    }
    line
}

/// A wave's tasks as small squares in a row, one each, coloured by state. A square of a
/// task that just came, by `came`, fades in.
pub fn squares(
    tasks: &[Task],
    wave: &[usize],
    came: Option<&HashMap<String, Instant>>,
    cx: &App,
) -> Div {
    let mut row = div().flex().flex_row().flex_wrap().gap_1();
    let mut moving = false;
    for &at in wave {
        let task = &tasks[at];
        let mut square = square(task, cx);
        let arriving = came
            .and_then(|came| came.get(&task.task_id))
            .and_then(|&since| {
                activity::arriving(since, ARRIVE, cx)
                    .or_else(|| (since > Instant::now()).then_some(0.))
            });
        if let Some(t) = arriving {
            square = square.opacity(t);
            moving = true;
        }
        row = match task.status == TaskStatus::Running && arriving.is_none() {
            true => row.child(breathe(square)),
            false => row.child(square),
        };
    }
    row.when(moving, |row| row.child(activity::next_frame()))
}

/// What runs breathes, slowly: dimmer and back once every `BREATH`.
pub fn breathe<E: IntoElement + Styled + 'static>(mark: E) -> impl IntoElement {
    mark.looping(BREATH, |mark, phase| {
        let deep = 0.5 - 0.5 * (phase * std::f32::consts::TAU).cos();
        mark.opacity(1. - 0.45 * deep)
    })
}

fn square(task: &Task, cx: &App) -> Div {
    mark_square(RunMark::of_task(task), cx)
}

/// Done is a quiet fill, running the accent; open is a hollow outline, waiting for a
/// decision an accent ring, failed the danger's fill, blocked the warning's ring. The
/// chat's run cards draw their tasks and agents with it too.
pub fn mark_square(mark: RunMark, cx: &App) -> Div {
    let theme = cx.theme();
    let square = div().flex_none().size(px(SQUARE)).rounded(px(2.5));
    match mark {
        RunMark::Done => square.bg(theme.foreground.opacity(0.26)),
        RunMark::Running => square.bg(theme.primary),
        RunMark::Failed => square.bg(theme.danger),
        RunMark::Waiting | RunMark::Confirm => square.border_1().border_color(theme.primary),
        RunMark::Blocked => square.border_1().border_color(theme.warning),
        RunMark::Open => square
            .border_1()
            .border_color(theme.foreground.opacity(0.3)),
    }
}

/// The waves of the shown chat's plan. `key` keeps two views' element IDs apart, `inset`
/// is where a wave's name starts, so the rows line up with the view around them.
pub struct View<'a> {
    pub key: &'static str,
    pub folds: &'a Folds,
    pub of: FoldsOf,
    pub came: Option<&'a HashMap<String, Instant>>,
    pub inset: f32,
    /// A hairline between waves, where the view has no other rhythm to part them.
    pub lines: bool,
}

pub fn render(ws: &Workspace, view: View, cx: &mut Context<Workspace>) -> Div {
    let Some((chat_id, chat)) = ws.shown.as_deref().zip(ws.shown_chat()) else {
        return div();
    };
    let tasks = chat.tasks.as_slice();
    let waves = team_layout::waves(tasks);
    let now = current(tasks, &waves);
    let mut list = div().flex().flex_col();
    for (index, wave) in waves.iter().enumerate() {
        let open = view.folds.wave_open(chat_id, index, now);
        list = list.child(wave_block(
            chat_id,
            tasks,
            &chat.agents,
            index,
            wave,
            open,
            &view,
            cx,
        ));
    }
    list
}

#[allow(clippy::too_many_arguments)]
fn wave_block(
    chat_id: &str,
    tasks: &[Task],
    agents: &std::collections::BTreeMap<String, Subagent>,
    index: usize,
    wave: &[usize],
    open: bool,
    view: &View,
    cx: &mut Context<Workspace>,
) -> Div {
    let theme = cx.theme();
    let (muted, hover) = (theme.muted_foreground, theme.list_hover);
    let line = theme.foreground.opacity(0.05);
    let done = wave
        .iter()
        .filter(|&&at| tasks[at].status == TaskStatus::Done)
        .count();
    let (of, chat) = (view.of, chat_id.to_owned());
    let head = div()
        .id(SharedString::from(format!("{}-wave-{index}", view.key)))
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .h(px(28.))
        .pl(px(view.inset - 24.))
        .pr(px(view.inset - 12.))
        .rounded_md()
        .cursor_pointer()
        .focusable_row(cx)
        .hover(move |head| head.bg(hover))
        .child(widgets::icon(
            if open {
                IconName::ChevronDown
            } else {
                IconName::ChevronRight
            },
            muted,
        ))
        .child(
            div()
                .flex_1()
                .small()
                .text_color(theme.foreground.opacity(0.8))
                .child(format!("Wave {}", index + 1)),
        )
        .child(
            div()
                .meta()
                .text_color(muted)
                .child(format!("{done}/{}", wave.len())),
        )
        .on_click(cx.listener(move |ws, _, _, cx| {
            // The click flips what shows now, whatever made it show.
            of(ws).mine(&chat).waves.insert(index, !open);
            cx.notify();
        }));
    let dots = div()
        .pl(px(view.inset))
        .pr(px(view.inset - 12.))
        .pt_1()
        .pb_2()
        .child(squares(tasks, wave, view.came, cx));
    let mut block = div()
        .flex()
        .flex_col()
        .when(view.lines && index > 0, |block| {
            block.mt_1().pt_1().border_t_1().border_color(line)
        })
        .child(head)
        .child(dots);
    if open {
        let mut rows = div().flex().flex_col().pb_2();
        for &at in wave {
            rows = rows.child(task_block(chat_id, &tasks[at], agents, view, cx));
        }
        block = block.child(widgets::unfold(
            SharedString::from(format!("{}-wave-body-{index}", view.key)),
            rows,
        ));
    }
    block
}

/// The run of the member that holds `task` last: when it started and ended.
pub fn run_of<'a>(
    task: &Task,
    agents: &'a std::collections::BTreeMap<String, Subagent>,
) -> Option<&'a Subagent> {
    agents
        .values()
        .filter(|agent| agent.task_id.as_deref() == Some(task.task_id.as_str()))
        .max_by_key(|agent| agent.started_at)
}

fn task_block(
    chat_id: &str,
    task: &Task,
    agents: &std::collections::BTreeMap<String, Subagent>,
    view: &View,
    cx: &mut Context<Workspace>,
) -> Div {
    let theme = cx.theme();
    let (muted, hover) = (theme.muted_foreground, theme.list_hover);
    let open = view.folds.task_open(chat_id, &task.task_id);
    let run = run_of(task, agents);
    // Only what needs a look carries a word, in its colour; the rest a time or nothing.
    let right = match task.status {
        TaskStatus::Failed => Some(("failed", theme.danger)),
        TaskStatus::Blocked => Some(("blocked", theme.warning)),
        _ if for_user(task) => Some(("confirm", theme.primary)),
        _ => None,
    };
    let right = match right {
        Some((word, color)) => Some(div().text_color(color).child(word)),
        None => match task.status {
            TaskStatus::Running => Some(
                div()
                    .text_color(theme.foreground.opacity(0.8))
                    .child("Running"),
            ),
            // A run with no length known says nothing rather than "0s".
            TaskStatus::Done => run
                .and_then(|run| run.ended_at.map(|end| end - run.started_at))
                .filter(|&took| took > 0)
                .map(|took| div().text_color(muted).child(duration_words(took))),
            _ => None,
        },
    };
    let (of, chat, id) = (view.of, chat_id.to_owned(), task.task_id.clone());
    let row = div()
        .id(SharedString::from(format!(
            "{}-task-{}",
            view.key, task.task_id
        )))
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .h(px(28.))
        .ml(px(view.inset - 26.))
        .mr(px(view.inset - 20.))
        .px_2()
        .rounded_md()
        .cursor_pointer()
        .focusable_row(cx)
        .hover(move |row| row.bg(hover))
        .child(mark(task.status, cx))
        .child(
            div()
                .flex_1()
                // In a narrow card the word on the right gives way first.
                .min_w(px(40.))
                .small()
                .truncate()
                .text_color(match task.status {
                    TaskStatus::Done => muted,
                    _ => theme.foreground.opacity(0.85),
                })
                .child(task.title()),
        )
        .children(right.map(|right| right.min_w_0().truncate().meta()))
        .on_click(cx.listener(move |ws, _, _, cx| {
            let folds = of(ws).mine(&chat);
            folds.task = match folds.task.as_deref() == Some(id.as_str()) {
                true => None,
                false => Some(id.clone()),
            };
            cx.notify();
        }));
    let block = div().flex().flex_col().child(row);
    if !open {
        return block;
    }
    block.child(widgets::unfold(
        SharedString::from(format!("{}-task-body-{}", view.key, task.task_id)),
        details(task, run, view, cx),
    ))
}

/// A task's state as a small mark before its words: a check once done, else a dot drawn
/// as its square is.
fn mark(status: TaskStatus, cx: &App) -> Div {
    let theme = cx.theme();
    let slot = div()
        .flex_none()
        .size(px(18.))
        .flex()
        .items_center()
        .justify_center();
    let dot = div().size(px(7.)).rounded_full();
    match status {
        TaskStatus::Done => {
            slot.child(widgets::icon(IconName::Check, theme.muted_foreground).size(px(13.)))
        }
        TaskStatus::Running => slot.child(breathe(dot.bg(theme.primary))),
        TaskStatus::Failed => slot.child(dot.bg(theme.danger)),
        TaskStatus::Claimed | TaskStatus::Review => {
            slot.child(dot.border_1().border_color(theme.primary))
        }
        TaskStatus::Blocked => slot.child(dot.border_1().border_color(theme.warning)),
        TaskStatus::Open => slot.child(dot.border_1().border_color(theme.foreground.opacity(0.3))),
    }
}

/// Who holds a task, as what, on which model and which try; its last note; and what can
/// be done: confirm it, open its member's chat, open its card.
fn details(task: &Task, run: Option<&Subagent>, view: &View, cx: &mut Context<Workspace>) -> Div {
    let theme = cx.theme();
    let muted = theme.muted_foreground;
    let model = task
        .model
        .as_deref()
        .or(run.and_then(|run| run.model.as_deref()))
        .map(short_model);
    let who = match &task.owner {
        Some(owner) => {
            let meta = [
                Some(role_name(task.role.as_deref()).to_owned()),
                model,
                (task.attempts > 0).then(|| format!("attempt {}", task.attempts)),
            ]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" \u{00B7} ");
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_2()
                        .text_color(theme.foreground.opacity(0.85))
                        .child(face(&owner.name, 18., cx))
                        .child(owner.name.clone()),
                )
                .child(div().text_color(muted).child(meta))
        }
        None => div().text_color(muted).child("Not given to a member yet"),
    };
    // The note is model or program text: drawn as plain text, a few lines at most.
    let note = (!task.note.trim().is_empty()).then(|| {
        div()
            .flex()
            .flex_row()
            .gap_2()
            .mt_2()
            .child(
                div()
                    .flex_none()
                    .w(px(2.))
                    .my(px(3.))
                    .rounded_full()
                    .bg(theme.foreground.opacity(0.12)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_color(muted)
                    .line_clamp(4)
                    .child(task.note.clone()),
            )
    });
    let key = view.key;
    let mut actions = div().flex().flex_row().flex_wrap().gap_1p5().mt_2p5();
    if for_user(task) {
        let id = task.task_id.clone();
        actions = actions.child(
            Button::new(SharedString::from(format!(
                "{key}-confirm-{}",
                task.task_id
            )))
            .small()
            .primary()
            .label("Confirm")
            .on_click(cx.listener(move |ws, _, _, cx| ws.confirm_task(&id, cx))),
        );
    }
    if let Some(owner) = &task.owner {
        let thread = owner.thread_id.clone();
        actions = actions.child(
            Button::new(SharedString::from(format!("{key}-chat-{}", task.task_id)))
                .small()
                .ghost()
                .label("Open transcript")
                .on_click(cx.listener(move |ws, _, _, cx| ws.open_agent_chat(&thread, cx))),
        );
    }
    let id = task.task_id.clone();
    actions = actions.child(
        Button::new(SharedString::from(format!("{key}-card-{}", task.task_id)))
            .small()
            .ghost()
            .label("Details")
            .on_click(cx.listener(move |ws, _, _, cx| ws.open_task_card(&id, cx))),
    );
    div()
        .flex()
        .flex_col()
        .pl(px(view.inset + 6.))
        .pr(px(view.inset - 12.))
        .pt_1()
        .pb_3()
        .small()
        .child(who)
        .children(note)
        .child(actions)
}

/// A member's face: the first letter of its name on a disc of the colour its name picks.
pub fn face(name: &str, size: f32, cx: &App) -> Div {
    let theme = cx.theme();
    div()
        .flex_none()
        .size(px(size))
        .rounded_full()
        .flex()
        .items_center()
        .justify_center()
        .bg(avatar::tone(avatar::shade(name), cx).opacity(0.85))
        .text_color(theme.background)
        .text_size(px(size * 0.52))
        .font_weight(FontWeight::BOLD)
        .child(avatar::letter(name))
}

/// The wave a task is in, for unfolding it from elsewhere.
pub fn wave_of(tasks: &[Task], task_id: &str) -> Option<usize> {
    let at = tasks.iter().position(|task| task.task_id == task_id)?;
    team_layout::waves(tasks)
        .iter()
        .position(|wave| wave.contains(&at))
}

/// How long a member has run its task: "6m", "now".
pub fn running_for(run: &Subagent) -> String {
    dusk_protocol::thread::age_words(dusk_protocol::system::now_ms() - run.started_at)
}
