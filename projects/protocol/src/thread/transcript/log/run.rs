//! The runs the chat started, kept as one item where each began (`transcript::run`).

use super::{ChatLog, Item};
use crate::{
    model::ToolCall,
    thread::{
        Team, TeamStatus,
        transcript::{Run, RunCard, RunKind, RunMark, Tool, ToolState, run::task_marks},
    },
    tools::ToolResult,
};

/// How much later than the step that asked for it the core may say a team started: the
/// user answering for the plan's gates comes between, and two clocks may differ a little.
const START_SLACK_MS: i64 = 5_000;

impl<L> ChatLog<L> {
    /// Takes in a call that starts a run: a team as an item of its own, an `ask_agent` into
    /// the subagents its step asked already. Whether it was one.
    pub(super) fn start_run(&mut self, call: &ToolCall, tool: Tool, at: i64) -> bool {
        let Some(kind) = Run::started_by(call) else {
            return false;
        };
        if kind == RunKind::Subagents
            && let Some(index) = self.batch
            && let Some(Item::Run(run)) = self.items.get_mut(index)
            && run.kind == RunKind::Subagents
        {
            run.tools.push(tool);
            self.touch(index);
            return true;
        }
        let index = self.push(
            Item::Run(Run {
                kind,
                tools: vec![tool],
            }),
            at,
        );
        self.batch = (kind == RunKind::Subagents).then_some(index);
        true
    }

    /// The run that holds the call `call_id`.
    pub(super) fn run_index(&self, call_id: &str) -> Option<usize> {
        self.items.iter().rposition(|item| {
            matches!(item, Item::Run(run) if run.tools.iter().any(|tool| tool.call_id == call_id))
        })
    }

    /// Ends a call of a run as `finish` says. A team that did not start is no run: its
    /// call goes back to being a step, with its error. Whether the call was a run's.
    pub(super) fn finish_run_call(
        &mut self,
        call_id: &str,
        finish: impl FnOnce(&mut Tool),
    ) -> bool {
        let Some(index) = self.run_index(call_id) else {
            return false;
        };
        let Item::Run(run) = &mut self.items[index] else {
            return false;
        };
        let kind = run.kind;
        let Some(tool) = run.tool_mut(call_id) else {
            return false;
        };
        finish(tool);
        if kind == RunKind::Team && tool.state == ToolState::Failed {
            let tool = tool.clone();
            self.items[index] = Item::Tool(tool);
            self.reshape();
        }
        self.touch(index);
        true
    }

    pub(super) fn finish_run(&mut self, call_id: &str, result: &ToolResult) -> bool {
        self.finish_run_call(call_id, |tool| tool.finish(result))
    }

    /// Every team run's card, when the team changed: a new run turns the last one into
    /// an earlier run.
    pub(super) fn touch_team_runs(&mut self) {
        let indexes: Vec<usize> = (0..self.items.len())
            .filter(
                |&index| matches!(&self.items[index], Item::Run(run) if run.kind == RunKind::Team),
            )
            .collect();
        for index in indexes {
            self.touch(index);
        }
    }

    /// The card of the subagents' run that holds the call `call_id`, when its agent changed.
    pub(super) fn touch_agent_run(&mut self, call_id: &str) {
        if let Some(index) = self.run_index(call_id) {
            self.touch(index);
        }
    }

    /// The card of the team's current run shows what the plan's tasks changed; earlier
    /// runs show none of it.
    pub(super) fn touch_team_run(&mut self) {
        let owner = match &self.team {
            Some(team) => self.team_owner(team),
            None => self
                .items
                .iter()
                .rposition(|item| matches!(item, Item::Run(run) if run.kind == RunKind::Team)),
        };
        if let Some(index) = owner {
            self.touch(index);
        }
    }

    /// The team run the chat's team is: the last one started no later than the core says
    /// the team did, else the last one. Those before it are earlier runs.
    fn team_owner(&self, team: &Team) -> Option<usize> {
        let runs = (0..self.items.len()).filter(
            |&index| matches!(&self.items[index], Item::Run(run) if run.kind == RunKind::Team),
        );
        let started = team.started_at;
        let mut last = None;
        let mut owner = None;
        for index in runs {
            last = Some(index);
            if started.is_none_or(|started| self.at(index).unwrap_or(0) <= started + START_SLACK_MS)
            {
                owner = Some(index);
            }
        }
        owner.or(last)
    }

    /// What the card of the run at `index` shows at `now`, Unix milliseconds of the
    /// client's clock.
    pub fn run_card(&self, index: usize, now: i64) -> Option<RunCard> {
        let Some(Item::Run(run)) = self.items.get(index) else {
            return None;
        };
        Some(match run.kind {
            RunKind::Team => self.team_card(run, index, now),
            RunKind::Subagents => self.subagents_card(run, now),
        })
    }

    fn team_card(&self, run: &Run, index: usize, now: i64) -> RunCard {
        // A plan has no name of its own: the chat's title names the work it plans, and
        // "Plan" stands in until the chat has one.
        let title = self.title.clone().unwrap_or_else(|| "Plan".to_owned());
        let team = self.team.as_ref();
        let earlier = team
            .and_then(|team| self.team_owner(team))
            .is_some_and(|owner| index < owner);
        let mut card = RunCard {
            kind: RunKind::Team,
            title,
            agents: 0,
            elapsed_ms: None,
            running: false,
            earlier,
            marks: Vec::new(),
        };
        if earlier {
            return card;
        }
        card.marks = task_marks(&self.tasks);
        let Some(team) = team else {
            // The team's own word comes just after the call that started it; a chat read
            // from history may never hear it, and its card does not run on for that.
            card.running = run
                .tools
                .iter()
                .any(|tool| tool.state == ToolState::Running);
            return card;
        };
        card.agents = team.members.len();
        card.running = team.status == TeamStatus::Running;
        let minutes = i64::from(team.used.minutes) * 60_000;
        card.elapsed_ms = Some(match (team.started_at, team.status) {
            (Some(started), TeamStatus::Running) => (now - started).max(0),
            (Some(started), TeamStatus::Ended(_)) => {
                team.ended_at.map_or(minutes, |ended| ended - started)
            }
            (None, _) => minutes,
        });
        card
    }

    fn subagents_card(&self, run: &Run, now: i64) -> RunCard {
        let agents: Vec<_> = run
            .tools
            .iter()
            .map(|tool| {
                let agent = self
                    .agents
                    .values()
                    .find(|agent| agent.call_id == tool.call_id);
                (tool, agent)
            })
            .collect();
        let marks: Vec<RunMark> = agents
            .iter()
            .map(|(tool, agent)| RunMark::of_agent(agent.map(|agent| &agent.state), tool.state))
            .collect();
        let known: Vec<_> = agents.iter().filter_map(|(_, agent)| *agent).collect();
        let running = known.iter().any(|agent| agent.working())
            || run
                .tools
                .iter()
                .any(|tool| tool.state == ToolState::Running);
        let elapsed_ms = known
            .iter()
            .map(|agent| agent.started_at)
            .min()
            .map(|started| {
                let ended = match running {
                    true => now,
                    false => known
                        .iter()
                        .filter_map(|agent| agent.ended_at)
                        .max()
                        .unwrap_or(now),
                };
                ended - started
            });
        let count = run.tools.len();
        let title = match run.tools.as_slice() {
            [one] if !one.detail.is_empty() => one.detail.clone(),
            _ if count == 1 => "1 subagent".to_owned(),
            _ => format!("{count} subagents"),
        };
        RunCard {
            kind: RunKind::Subagents,
            title,
            agents: count,
            elapsed_ms,
            running,
            earlier: false,
            marks,
        }
    }
}
