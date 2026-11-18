//! The dock between the transcript and the composer: the plan strip, one line that unfolds
//! to the plan's rows, and the views tab on the composer's upper edge with its menu. Both
//! draw what the core holds: the chat's tasks and the plan's counts.

mod plan;
mod views;

pub use plan::render as plan_strip;
pub use views::render as views_tab;

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
    pub views_open: bool,
    /// The chosen row of the views menu.
    pub views_chosen: usize,
    /// The views tab, a stop of the focus order.
    pub views_focus: FocusHandle,
    /// The open menu, which takes the keys.
    pub views_menu_focus: FocusHandle,
}

impl Dock {
    pub fn new(cx: &mut App) -> Self {
        Self {
            counts: None,
            busy: false,
            queued: false,
            plan_open: false,
            rows: UniformListScrollHandle::new(),
            views_open: false,
            views_chosen: 0,
            views_focus: cx.focus_handle().tab_index(0).tab_stop(true),
            views_menu_focus: cx.focus_handle(),
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

    pub fn toggle_plan(&mut self, cx: &mut Context<Self>) {
        self.dock.plan_open = !self.dock.plan_open;
        cx.notify();
    }
}
