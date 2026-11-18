//! The Overview: how the shown chat's work stands. One module with two widths: the right
//! column draws its sections in one column, "Open" draws them as a page in two. Every
//! number is the core's: the plan's counts, the team, the token report, the context and
//! the core's Overview; only elapsed time and layout are the client's.

mod page;
pub mod sections;

use std::time::{Duration, Instant};

use dusk_protocol::thread::{
    EVENTS_PAGE, LogEntry, OVERVIEW_LOG, Overview, PlanCounts, Team, ThreadEvent,
};
use gpui_kit::*;

use crate::workspace::{Center, Right, Workspace};

pub use page::render as page;
pub use sections::Form;

/// The Overview asks the core at most this often.
const SPACING: Duration = Duration::from_secs(1);

/// What the Overview last heard from the core, for the chat in `chat`.
#[derive(Default)]
pub struct Shown {
    pub chat: Option<String>,
    pub overview: Option<Overview>,
    pub plan: Option<PlanCounts>,
    pub team: Option<Team>,
    /// Log lines older than the Overview's, read page by page with "All events".
    pub older: Vec<LogEntry>,
    /// The last page was full, so older lines may be left.
    pub more: bool,
    /// A request is on its way or waits for its second.
    busy: bool,
    /// Something changed while busy: ask again once the answer is in.
    queued: bool,
    asked_at: Option<Instant>,
}

impl Shown {
    /// The answers when they are of `chat`; another chat's are not shown.
    pub fn of(&self, chat: Option<&str>) -> Option<&Self> {
        (self.chat.as_deref() == chat && self.overview.is_some()).then_some(self)
    }
}

impl Workspace {
    /// Whether the Overview is drawn now, as the module or the page.
    pub fn overview_open(&self) -> bool {
        let module = !self.right_hidden && !self.folded.contains(&Right::Overview);
        module || self.center == Center::Overview
    }

    /// Opens the Overview as a page.
    pub fn open_overview(&mut self, cx: &mut Context<Self>) {
        self.center = Center::Overview;
        self.load_context(cx);
        self.load_overview(cx);
        cx.notify();
    }

    /// A turn ended or the plan or team changed: the open Overview asks again.
    pub fn overview_heard(&mut self, thread_id: &str, event: &ThreadEvent, cx: &mut Context<Self>) {
        let moved = matches!(
            event,
            ThreadEvent::TurnFinished(_) | ThreadEvent::TaskChanged(_) | ThreadEvent::Team(_)
        );
        if moved && self.overview_open() && self.shown.as_deref() == Some(thread_id) {
            self.load_overview(cx);
        }
    }

    /// Asks the core for the Overview, at most once a second.
    pub fn load_overview(&mut self, cx: &mut Context<Self>) {
        if self.overview.busy {
            self.overview.queued = true;
            return;
        }
        let since = self.overview.asked_at.map_or(SPACING, |at| at.elapsed());
        let Some(wait) = SPACING.checked_sub(since).filter(|wait| !wait.is_zero()) else {
            return self.fetch_overview(cx);
        };
        self.overview.busy = true;
        cx.spawn(async move |ws, cx| {
            cx.background_executor().timer(wait).await;
            let _ = ws.update(cx, |ws, cx| {
                ws.overview.busy = false;
                ws.fetch_overview(cx);
            });
        })
        .detach();
    }

    fn fetch_overview(&mut self, cx: &mut Context<Self>) {
        let Some(thread_id) = self.shown.clone() else {
            return;
        };
        self.overview.busy = true;
        self.overview.asked_at = Some(Instant::now());
        let asked = thread_id.clone();
        self.core.call(
            cx,
            move |client| async move {
                let work = client.work();
                let overview = work.overview(&thread_id).await?;
                let plan = work.plan_tasks(&thread_id, 1).await?.counts;
                let team = work.team(&thread_id).await?;
                Ok((overview, plan, team))
            },
            move |ws, result, cx| {
                ws.overview.busy = false;
                match result {
                    Ok((overview, plan, team)) if ws.shown.as_deref() == Some(asked.as_str()) => {
                        let same = ws.overview.chat.as_deref() == Some(asked.as_str())
                            && !ws.overview.older.is_empty();
                        let full = overview.log.len() >= OVERVIEW_LOG;
                        ws.overview = Shown {
                            chat: Some(asked),
                            overview: Some(overview),
                            plan: Some(plan),
                            team,
                            // Older pages stay while the chat is the same.
                            older: if same {
                                std::mem::take(&mut ws.overview.older)
                            } else {
                                Vec::new()
                            },
                            more: if same { ws.overview.more } else { full },
                            busy: false,
                            queued: ws.overview.queued,
                            asked_at: ws.overview.asked_at,
                        };
                        cx.notify();
                    }
                    // Another chat is shown by now: ask for its own.
                    Ok(_) => ws.overview.queued = true,
                    Err(error) => ws.fail(error, cx),
                }
                if std::mem::take(&mut ws.overview.queued) {
                    ws.load_overview(cx);
                }
            },
        );
    }

    /// Reads the next page of the log, older than the oldest line shown.
    pub fn load_older_events(&mut self, cx: &mut Context<Self>) {
        let Some(thread_id) = self.shown.clone() else {
            return;
        };
        let Some(shown) = self.overview.of(Some(&thread_id)) else {
            return;
        };
        let newest = shown.overview.as_ref().map(|o| o.log.as_slice());
        let before = shown
            .older
            .last()
            .or_else(|| newest.and_then(|log| log.last()))
            .map(|entry| entry.at);
        let asked = thread_id.clone();
        self.core.call(
            cx,
            move |client| async move {
                client
                    .work()
                    .events(&thread_id, before, EVENTS_PAGE)
                    .await
            },
            move |ws, result, cx| match result {
                Ok(page) if ws.overview.chat.as_deref() == Some(asked.as_str()) => {
                    ws.overview.more = page.len() == EVENTS_PAGE as usize;
                    ws.overview.older.extend(page);
                    cx.notify();
                }
                Ok(_) => {}
                Err(error) => ws.fail(error, cx),
            },
        );
    }
}
