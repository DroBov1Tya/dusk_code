//! The dock between the transcript and the composer: the plan strip, one line that unfolds
//! to the plan's rows, and the panel the tab on the composer's upper edge opens. Both draw
//! what the core holds: the chat's tasks, the plan's counts, the chat's folder and meters.

pub mod panel;
mod plan;

pub use plan::render as plan_strip;

use dusk_protocol::thread::{PlanCounts, ThreadEvent};
use gpui_kit::*;

use crate::workspace::Workspace;

/// The dock's own state; folds are the window's, never the core's.
pub struct Dock {
    /// The plan's counts as the core last gave them, and the chat they are of.
    pub counts: Option<(String, PlanCounts)>,
    /// A request for the counts is on its way.
    busy: bool,
    /// The plan moved while busy: ask again once the answer is in.
    queued: bool,
    /// The strip shows the plan's rows.
    pub plan_open: bool,
    pub rows: UniformListScrollHandle,
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
            busy: false,
            queued: false,
            plan_open: false,
            rows: UniformListScrollHandle::new(),
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
    /// The shown chat's tasks changed: the strip asks for the plan's counts again.
    pub fn plan_heard(&mut self, thread_id: &str, event: &ThreadEvent, cx: &mut Context<Self>) {
        let moved = matches!(event, ThreadEvent::Tasks(_) | ThreadEvent::TaskChanged(_));
        if moved && self.shown.as_deref() == Some(thread_id) {
            self.load_plan_counts(cx);
        }
    }

    /// Asks the core for the shown chat's plan counts; a burst of changes asks once more
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
            move |client| async move { Ok(client.work().plan_tasks(&thread_id, 1).await?.counts) },
            move |ws, result, cx| {
                ws.dock.busy = false;
                match result {
                    Ok(counts) => {
                        ws.dock.counts = Some((asked, counts));
                        cx.notify();
                    }
                    // The strip keeps the last counts; the board still draws its rows.
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
