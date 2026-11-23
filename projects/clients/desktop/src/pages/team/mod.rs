//! The Team page: the team that runs the shown chat's plan, drawn as two layers that
//! share one selection. **People** is the lead and its members with who gave what to
//! whom; **Plan** is the tasks in waves by what they need. The layout is the protocol's
//! (`team_layout`); this module only draws it, and the narrow module of the right column
//! (`module`) shows the same team in rows.
//! - `page`: the page, its header, budget and the layers;
//! - `graph`: what both layers draw with: a box at its place, the edges, a member's mark;
//! - `people`, `plan`: the two layers;
//! - `module`: the right column's Team screen;
//! - `waves`: the plan in waves with a square a task, shared by the chat's plan card and
//!   the Team screen.

mod graph;
pub mod module;
mod page;
mod people;
mod plan;
pub mod waves;

pub use page::render;

use dusk_protocol::thread::team_layout::Pick;
use gpui_kit::*;

use crate::workspace::{Center, Workspace};

/// Which layers the page shows.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum Layer {
    #[default]
    Both,
    People,
    Plan,
}

/// What the page keeps of its own; the team and the plan are the chat's.
#[derive(Default)]
pub struct TeamPage {
    pub layer: Layer,
    /// The node under the pointer: both layers answer it.
    pub hover: Option<Pick>,
    /// The node clicked last; it lights while nothing is under the pointer.
    pub picked: Option<Pick>,
    /// A long plan shows its done waves.
    pub unfold: bool,
    /// The page Back returns to.
    pub back: Option<Center>,
}

impl TeamPage {
    /// What both layers light now.
    pub fn pick(&self) -> Option<&Pick> {
        self.hover.as_ref().or(self.picked.as_ref())
    }
}

impl Workspace {
    /// Opens the Team page of the shown chat, and asks the core for its team and plan
    /// counts once; their events keep it current from here.
    pub fn open_team(&mut self, cx: &mut Context<Self>) {
        if self.center != Center::Team {
            self.team_page.back = Some(self.center);
        }
        self.center = Center::Team;
        self.team_page.hover = None;
        self.team_page.picked = None;
        self.load_plan_counts(cx);
        cx.notify();
    }

    /// Leaves the Team page for the page it was opened from.
    pub fn close_team(&mut self, cx: &mut Context<Self>) {
        if self.center == Center::Team {
            self.center = self.team_page.back.take().unwrap_or(Center::Chat);
            cx.notify();
        }
    }

    /// The pointer came onto a node, or left it.
    fn team_hover(&mut self, pick: Pick, inside: bool, cx: &mut Context<Self>) {
        let hover = &mut self.team_page.hover;
        match inside {
            true => *hover = Some(pick),
            false if hover.as_ref() == Some(&pick) => *hover = None,
            false => return,
        }
        cx.notify();
    }

    /// The module's pick lasts only as long as what it opened; the page keeps its own
    /// until it is opened again.
    pub fn team_unpick(&mut self, cx: &mut Context<Self>) {
        if self.center != Center::Team && self.team_page.picked.take().is_some() {
            cx.notify();
        }
    }

    /// A click on a member opens its chat, a click on a task its card; both stay lit.
    fn team_open(&mut self, pick: Pick, cx: &mut Context<Self>) {
        match &pick {
            Pick::Member(thread_id) => self.open_agent_chat(thread_id, cx),
            Pick::Task(task_id) => self.open_task_card(task_id, cx),
        }
        self.team_page.picked = Some(pick);
        cx.notify();
    }
}
