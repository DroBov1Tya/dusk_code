//! Tabs: threads kept open side by side, like a terminal's tabs. The shown tab's thread
//! lives in the `App` fields as before; every other tab keeps the same fields in a `Tab`
//! and trades them with the `App` to be shown or to take one of its events. Which tabs
//! are open is presentation, kept nowhere else.

use std::{
    collections::{HashSet, VecDeque},
    time::Instant,
};

use dusk_core::app::ArtifactStore;
use dusk_protocol::{
    thread::{Goal, JobInfo, LocalUnsent, Task, ThreadTokens},
    tools::QuestionQueue,
};

use super::{Activity, App, Asked, Focus, Input, Poll, SubagentView, Transcript};

/// What a tab key or `/tab` asks for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TabMove {
    New,
    Close,
    Next,
    Previous,
    /// The tab at this place on the strip, from 0.
    Go(usize),
}

impl TabMove {
    /// `/tab`, `/tab close`, `/tab next`, `/tab prev` or `/tab 2`.
    pub fn parse(argument: &str) -> Option<Self> {
        match argument.trim() {
            "" | "new" => Some(Self::New),
            "close" => Some(Self::Close),
            "next" => Some(Self::Next),
            "prev" | "previous" => Some(Self::Previous),
            number => match number.parse::<usize>() {
                Ok(place @ 1..) => Some(Self::Go(place - 1)),
                _ => None,
            },
        }
    }
}

/// A tab on the strip on the last drawn frame, by its place on the strip.
pub struct TabHit {
    pub row: u16,
    pub left: u16,
    pub right: u16,
    pub place: usize,
}

/// Declares `Tab` with the per-thread fields of `App`, and the trade of the two. A field
/// of `App` that belongs to the thread on screen is listed here, or another tab's thread
/// would show it.
macro_rules! per_thread {
    ($($field:ident: $type:ty,)*) => {
        /// A tab's thread while another one shows.
        #[derive(Default)]
        pub struct Tab {
            $(pub $field: $type,)*
            /// It finished in the background and has not been looked at since.
            pub unseen: bool,
            /// Closed while its agent worked: off the strip until the work ends, or back
            /// on it when the agent asks something.
            pub closed: bool,
        }

        impl Tab {
            fn trade(&mut self, app: &mut App) {
                $(std::mem::swap(&mut self.$field, &mut app.$field);)*
            }
        }
    };
}

per_thread! {
    thread_id: Option<String>,
    temporary_next: bool,
    temporary: bool,
    title: Option<String>,
    transcript: Transcript,
    input: Input,
    focus: Focus,
    activity: Activity,
    tokens: ThreadTokens,
    scroll: usize,
    drawn_size: (usize, usize),
    queued: Vec<String>,
    forked: Option<String>,
    undone: Option<String>,
    poll: Option<Poll>,
    questions: QuestionQueue,
    suggestion: dusk_protocol::thread::Suggestion,
    approvals: Vec<Asked>,
    local_unsent: Vec<LocalUnsent>,
    jobs: Vec<JobInfo>,
    jobs_stale: bool,
    subagents: Vec<SubagentView>,
    tasks: Vec<Task>,
    unfolded_tasks: Vec<String>,
    task_scroll: usize,
    agent_scroll: usize,
    goal: Option<Goal>,
    turn_started_at: Option<i64>,
    last_turn_duration_ms: Option<i64>,
    turn_spend: Option<dusk_protocol::thread::TurnSpend>,
    turn_running: bool,
    compacting: bool,
    sent: Vec<String>,
    sent_ids: HashSet<String>,
    recall: Option<usize>,
    recall_draft: Option<Input>,
    output_samples: VecDeque<(Instant, usize)>,
}

impl Tab {
    /// Its agent, or one it asked, is at work.
    pub fn working(&self) -> bool {
        self.activity != Activity::Idle
            || self.turn_running
            || self.compacting
            || self.subagents.iter().any(SubagentView::is_working)
    }

    /// The agent waits for an approval or an answer.
    pub fn asks(&self) -> bool {
        !self.approvals.is_empty() || self.poll.is_some()
    }
}

impl App {
    /// The shown thread's agent, or one it asked, is at work.
    pub fn working(&self) -> bool {
        self.is_busy() || self.turn_running || self.compacting || self.agents_at_work()
    }

    /// Any tab not on screen is at work, closed ones too.
    pub fn tabs_working(&self) -> bool {
        self.tabs.iter().any(Tab::working)
    }

    /// The tabs on the strip, in order, by index into `tabs`.
    pub fn strip(&self) -> Vec<usize> {
        (0..self.tabs.len())
            .filter(|&index| !self.tabs[index].closed)
            .collect()
    }

    /// The tab that holds `thread_id` while another shows.
    pub fn tab_of(&self, thread_id: &str) -> Option<usize> {
        self.tabs
            .iter()
            .position(|tab| tab.thread_id.as_deref() == Some(thread_id))
    }

    /// Trades tab `index`'s thread with the shown one. Called in pairs: once to bring the
    /// tab's thread in, once more to put both back.
    pub fn trade_tab(&mut self, index: usize) {
        let mut tab = std::mem::take(&mut self.tabs[index]);
        tab.trade(self);
        self.tabs[index] = tab;
    }

    /// Runs `apply` with tab `index`'s thread in place of the shown one.
    pub fn in_tab<R>(&mut self, index: usize, apply: impl FnOnce(&mut App) -> R) -> R {
        self.trade_tab(index);
        let result = apply(self);
        self.trade_tab(index);
        result
    }

    pub fn new_tab(&mut self) {
        self.tabs.push(self.fresh_tab());
        self.select_tab(self.tabs.len() - 1);
    }

    /// A tab for a new thread, its pictures read from the core's store.
    fn fresh_tab(&self) -> Tab {
        let mut tab = Tab::default();
        let store = ArtifactStore::new(self.status.artifacts_dir.clone());
        tab.transcript.set_artifact_store(store);
        tab
    }

    pub fn step_tab(&mut self, forward: bool) {
        let strip = self.strip();
        let Some(at) = strip.iter().position(|&index| index == self.tab) else {
            return;
        };
        let next = match forward {
            true => (at + 1) % strip.len(),
            false => (at + strip.len() - 1) % strip.len(),
        };
        self.select_tab(strip[next]);
    }

    /// Shows the tab at `place` on the strip.
    pub fn go_tab(&mut self, place: usize) {
        match self.strip().get(place) {
            Some(&index) => self.select_tab(index),
            None => self.flash = Some(format!("no tab {}", place + 1)),
        }
    }

    pub fn select_tab(&mut self, index: usize) {
        if index == self.tab || self.tabs.get(index).is_none_or(|tab| tab.closed) {
            return;
        }
        self.trade_tab(self.tab);
        self.show_tab(index);
    }

    /// Closes the shown tab, keeping its thread off the strip while `keep`; the last tab
    /// gives way to a new thread. False when there is nothing to close.
    pub fn close_tab(&mut self, keep: bool) -> bool {
        let strip = self.strip();
        let at = strip.iter().position(|&index| index == self.tab);
        let neighbour = at.and_then(|at| {
            strip
                .get(at + 1)
                .or(at.checked_sub(1).and_then(|before| strip.get(before)))
        });
        if neighbour.is_none() && self.thread_id.is_none() {
            self.flash = Some("a new thread is all this tab holds".into());
            return false;
        }
        let closing = self.tab;
        self.trade_tab(closing);
        let mut neighbour = neighbour.copied();
        if keep {
            self.tabs[closing].closed = true;
        } else {
            self.tabs.remove(closing);
            neighbour = neighbour.map(|index| index - usize::from(index > closing));
        }
        let index = neighbour.unwrap_or_else(|| {
            self.tabs.push(self.fresh_tab());
            self.tabs.len() - 1
        });
        self.show_tab(index);
        true
    }

    /// Drops the closed tabs whose work has ended; their threads are for the session to
    /// close.
    pub fn forget_closed(&mut self) -> Vec<String> {
        let mut gone = Vec::new();
        let mut index = 0;
        while index < self.tabs.len() {
            let tab = &self.tabs[index];
            if index == self.tab || !tab.closed || tab.working() || tab.asks() {
                index += 1;
                continue;
            }
            gone.extend(self.tabs.remove(index).thread_id);
            if index < self.tab {
                self.tab -= 1;
            }
        }
        gone
    }

    /// Brings in tab `index`, whose slot holds the hollow the shown one left.
    fn show_tab(&mut self, index: usize) {
        self.trade_tab(index);
        self.tab = index;
        self.tabs[index].unseen = false;
        self.tabs[index].closed = false;
        self.jobs_stale = true;
        self.forget_overview();
        // What the last frame drew belongs to the thread shown before.
        self.layout = None;
        self.tail_text.clear();
        self.entry_lines.clear();
        self.drawn_top = 0;
        self.transcript_hits.clear();
        self.button_hits.clear();
        self.previews.clear();
        self.hover = None;
        self.selection = None;
        self.command_popup = None;
        self.paths = None;
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::app::Entry;
    use dusk_protocol::{
        client::{ChatGpt, Status},
        config::ApprovalPolicy,
        model::ModelEvent,
        thread::ThreadEvent,
    };

    pub(crate) fn app() -> App {
        App::new(Status {
            model: None,
            discovering_model: false,
            input_limit: 0,
            context_policy: None,
            effort: None,
            mode: String::new(),
            approvals: ApprovalPolicy::Ask,
            idle_screen_minutes: 0,
            notifications: false,
            sound_notifications: false,
            mcp: Vec::new(),
            mcp_starting: false,
            plugins: Vec::new(),
            available_update: None,
            install: None,
            updated_builtin: Vec::new(),
            chatgpt: ChatGpt::default(),
            chatgpt_model: false,
            plugin_parts: Default::default(),
            limits: None,
            provider_health: Vec::new(),
            artifacts_dir: Default::default(),
            workspace_dir: Default::default(),
        })
    }

    fn answers(app: &App) -> Vec<&str> {
        app.transcript
            .entries
            .iter()
            .filter_map(|entry| match entry {
                Entry::Answer(text) => Some(text.as_str()),
                _ => None,
            })
            .collect()
    }

    fn text(text: &str) -> ThreadEvent {
        ThreadEvent::Model(ModelEvent::Text(text.into()))
    }

    /// A tab in the background takes its thread's events, and has them when shown again;
    /// the shown thread never sees them.
    #[test]
    fn a_background_tab_keeps_its_events() {
        let mut app = app();
        app.thread_id = Some("a".into());
        app.apply(text("first"));
        app.new_tab();
        app.thread_id = Some("b".into());
        assert_eq!(app.strip().len(), 2);

        let index = app.tab_of("a").expect("thread a has a tab");
        app.in_tab(index, |app| {
            app.apply(text(" and more"));
            app.apply(ThreadEvent::TitleChanged { title: "A".into() });
        });
        assert_eq!(app.thread_id.as_deref(), Some("b"));
        assert!(answers(&app).is_empty());
        assert_eq!(app.title, None);

        app.go_tab(0);
        assert_eq!(app.thread_id.as_deref(), Some("a"));
        assert_eq!(answers(&app), ["first and more"]);
        assert_eq!(app.title.as_deref(), Some("A"));
        assert_eq!(app.tab_of("b"), Some(1));
    }

    /// A tab closed while its agent works stays off the strip until the work ends.
    #[test]
    fn a_closed_tab_waits_for_its_work_to_end() {
        let mut app = app();
        app.thread_id = Some("a".into());
        app.new_tab();
        app.thread_id = Some("b".into());
        app.go_tab(0);
        app.activity = Activity::Thinking;

        assert!(app.close_tab(app.working()));
        assert_eq!(app.thread_id.as_deref(), Some("b"));
        assert_eq!(app.strip().len(), 1);
        assert!(app.forget_closed().is_empty());

        let index = app.tab_of("a").expect("the closed tab is kept");
        app.in_tab(index, |app| app.apply(ThreadEvent::Idle));
        assert_eq!(app.forget_closed(), ["a"]);
        assert_eq!(app.tab_of("a"), None);
        assert_eq!(app.thread_id.as_deref(), Some("b"));
    }

    #[test]
    fn a_retry_is_one_footer_state_that_leaves_no_line_and_goes_with_the_answer() {
        let retrying = |attempt| ThreadEvent::Retrying {
            attempt,
            error: "timeout".into(),
            kind: dusk_protocol::model::ProviderErrorKind::Overloaded,
            wait_ms: 1000,
            max_attempts: Some(3),
        };
        let mut app = app();
        let lines = app.transcript.entries.len();
        app.apply(retrying(1));
        app.apply(retrying(2));
        assert!(matches!(&app.activity, Activity::Retrying(retry, _) if retry.attempt == 2));
        assert_eq!(app.transcript.entries.len(), lines);

        app.apply(text("hi"));
        assert!(app.activity == Activity::Writing);
    }
}
