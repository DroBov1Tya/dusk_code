//! The dock between the transcript and the composer: the plan card, a row a wave that
//! unfolds to its tasks, the standing goal, and the panel the tab on the composer's upper
//! edge opens. All draw what the core holds: the chat's tasks, the plan's counts, its goal,
//! the chat's folder and meters.

mod goal;
pub mod panel;
mod plan;

pub use goal::render as goal_strip;
pub use plan::render as plan_strip;

use dusk_protocol::thread::{PlanCounts, ThreadEvent};
use gpui_kit::*;

use crate::workspace::Workspace;

/// The dock's own state; folds are the window's, never the core's.
pub struct Dock {
    /// The plan's counts as the core last gave them, and the chat they are of.
    pub counts: Option<(String, PlanCounts)>,
    /// When each task of the shown chat's plan was first heard of, and the chat: a task's
    /// square that just came draws in once.
    pub tasks_came: (
        Option<String>,
        std::collections::HashMap<String, std::time::Instant>,
    ),
    /// A request for the counts is on its way.
    busy: bool,
    /// The plan moved while busy: ask again once the answer is in.
    queued: bool,
    /// The card shows the plan's waves.
    pub plan_open: bool,
    /// The goal's indicator is open to its card.
    pub goal_open: bool,
    /// Which waves and task of the card are unfolded.
    pub waves: crate::pages::team::waves::Folds,
    /// The composer's panel is open.
    pub panel_open: bool,
    /// The panel's tab, a stop of the focus order.
    pub panel_focus: FocusHandle,
    /// The pointer is over the composer's box or its tab: the tab is in sight.
    pub box_hovered: bool,
    /// The tab was in sight at least once, so it has something to fade from.
    pub tab_seen: bool,
}

impl Dock {
    pub fn new(cx: &mut App) -> Self {
        Self {
            counts: None,
            tasks_came: Default::default(),
            busy: false,
            queued: false,
            plan_open: true,
            goal_open: false,
            waves: Default::default(),
            panel_open: false,
            panel_focus: cx.focus_handle().tab_index(0).tab_stop(true),
            box_hovered: false,
            tab_seen: false,
        }
    }

    /// The counts when they are of `chat`.
    pub fn counts_of(&self, chat: Option<&str>) -> Option<PlanCounts> {
        let (of, counts) = self.counts.as_ref()?;
        (Some(of.as_str()) == chat).then_some(*counts)
    }
}

impl Workspace {
    /// The shown chat's tasks changed: the card asks for the plan's counts again.
    pub fn plan_heard(&mut self, thread_id: &str, event: &ThreadEvent, cx: &mut Context<Self>) {
        let tasks = match event {
            ThreadEvent::Tasks(tasks) => tasks.as_slice(),
            ThreadEvent::TaskChanged(task) => std::slice::from_ref(task),
            _ => return,
        };
        if self.shown.as_deref() == Some(thread_id) {
            self.tasks_heard(thread_id, tasks.iter().map(|task| task.task_id.as_str()));
            self.load_plan_counts(cx);
        }
    }

    /// Notes when each of `tasks` was first heard of. Those of one event come one after
    /// another, a little apart; the plan a chat is opened with is simply there.
    fn tasks_heard<'a>(&mut self, thread_id: &str, tasks: impl Iterator<Item = &'a str>) {
        use std::time::{Duration, Instant};
        let (chat, came) = &mut self.dock.tasks_came;
        let now = Instant::now();
        let opened = chat.as_deref() != Some(thread_id);
        if opened {
            *chat = Some(thread_id.to_owned());
            came.clear();
        }
        let mut fresh = 0;
        for task in tasks {
            if came.contains_key(task) {
                continue;
            }
            let at = match opened {
                true => now
                    .checked_sub(crate::pages::team::waves::ARRIVE)
                    .unwrap_or(now),
                false => now + Duration::from_millis(70) * fresh,
            };
            came.insert(task.to_owned(), at);
            fresh += 1;
        }
    }

    /// Asks the core for the shown chat's plan counts and team; a burst of changes asks once more
    /// after the answer, never twice at a time.
    pub fn load_plan_counts(&mut self, cx: &mut Context<Self>) {
        if self.dock.busy {
            self.dock.queued = true;
            return;
        }
        let Some(thread_id) = self.shown.clone() else {
            return;
        };
        self.dock.busy = true;
        let asked = thread_id.clone();
        self.core.call(
            cx,
            move |client| async move {
                let work = client.work();
                let counts = work.plan_tasks(&thread_id, 1).await?.counts;
                Ok((counts, work.team(&thread_id).await?))
            },
            move |ws, result, cx| {
                ws.dock.busy = false;
                match result {
                    Ok((counts, team)) => {
                        // The card says how the team stands, in a chat opened after it
                        // started too; its events keep it current from here, and an
                        // answer older than the last of them is dropped.
                        if let (Some(team), Some(chat)) = (team, ws.chats.get_mut(&asked)) {
                            chat.team_answer(team, dusk_protocol::system::now_ms());
                        }
                        ws.dock.counts = Some((asked, counts));
                        cx.notify();
                    }
                    // The card keeps the last counts; the board still draws its rows.
                    Err(error) => tracing::warn!("cannot read the plan's counts: {error:#}"),
                }
                if std::mem::take(&mut ws.dock.queued) {
                    ws.load_plan_counts(cx);
                }
            },
        );
    }

    /// Passes a task that waits for the user's confirmation. The core answers with the
    /// task's own event, which moves the row.
    pub fn confirm_task(&mut self, task_id: &str, cx: &mut Context<Self>) {
        let Some(thread_id) = self.shown.clone() else {
            return;
        };
        let task_id = task_id.to_owned();
        self.core.call(
            cx,
            move |client| async move { client.work().confirm_task(&thread_id, &task_id).await },
            |ws, result, cx| {
                if let Err(error) = result {
                    ws.fail(error, cx);
                }
            },
        );
    }

    pub fn toggle_plan(&mut self, cx: &mut Context<Self>) {
        self.dock.plan_open = !self.dock.plan_open;
        cx.notify();
    }
}
