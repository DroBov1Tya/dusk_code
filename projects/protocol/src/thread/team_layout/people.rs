//! The People layer, layered: the lead on the left, a column of its members to the right,
//! one edge each from the lead, and a dashed arc between two members when one waits on
//! the other's task. Members at work, in review, blocked or failed are full nodes; a done
//! one shrinks to a line, and past a few they fold into one chip per role.

use std::collections::{BTreeMap, HashMap};

use super::{Arrow, Edge, EdgeLabel, EdgeTone, Ends, Rect, across, arc, member::MemberState};
use crate::thread::{
    Subagent, Task, TaskStatus, Team, TeamEnd, WaitsOn, find_task, plural, short_model,
};

/// Where the member column starts and how wide it is.
const MEMBERS_X: f32 = 356.;
const MEMBER_W: f32 = 330.;
const CARD_H: f32 = 66.;
const LINE_H: f32 = 28.;
const LINE_STEP: f32 = 34.;
const CARD_GAP: f32 = 8.;
const LEAD_W: f32 = 236.;
const LEAD_H: f32 = 66.;
const CHIP_W: f32 = 162.;
const CHIP_H: f32 = 26.;
const CHIP_STEP: f32 = 32.;
/// The longest words a half-width chip holds.
const CHIP_CHARS: usize = 16;
const LABEL_W: f32 = 144.;
const WAIT_LABEL_W: f32 = 160.;
/// The layer's width with room for an arc's words beside the member column.
const WIDTH: f32 = 888.;
/// Done members past this many fold by role; so do they once the column passes `FOLD_ALL`.
const DONE_SHOWN: usize = 4;
const FOLD_ALL: usize = 9;

/// A member as a node of the layer and a row of the narrow module.
#[derive(Debug, Clone, PartialEq)]
pub struct MemberNode {
    /// Its thread, whose chat a click opens.
    pub thread_id: String,
    pub name: String,
    pub role: String,
    /// The model without its provider.
    pub model: Option<String>,
    pub task_id: String,
    /// Its task's number in the plan, when the plan holds it.
    pub number: Option<usize>,
    /// "#3 Remove expired entries in get()".
    pub task: Option<String>,
    /// "31 steps · 204k tokens", once its task spent anything.
    pub spend: Option<String>,
    pub state: MemberState,
    /// What it does, in a line.
    pub line: String,
    /// One or two words for a narrow row.
    pub word: &'static str,
    /// The narrow row's second line: what it waits on, does, or spent.
    pub narrow: String,
    /// Done: drawn as one line, with no words of what it does.
    pub one_line: bool,
    pub rect: Rect,
}

/// Members folded into one chip: the done ones of a role, those that failed and whose
/// task went to another, or those stopped.
#[derive(Debug, Clone, PartialEq)]
pub struct FoldChip {
    /// The status whose glyph leads it.
    pub status: TaskStatus,
    /// "backend-engineer ×7", "2 failed, given again".
    pub text: String,
    /// Who is in it, a line each: "Ember-11 · #1 Plan the cache layout".
    pub names: Vec<String>,
    pub rect: Rect,
}

/// The People layer.
#[derive(Debug, Clone, PartialEq)]
pub struct People {
    pub lead: Rect,
    /// "2 at work · 1 in review · 1 blocked", or how the team ended.
    pub lead_line: String,
    /// "3 of 8 tasks done".
    pub lead_tasks: String,
    /// The full nodes first, in their plan order, then the done ones drawn as a line.
    pub members: Vec<MemberNode>,
    pub folds: Vec<FoldChip>,
    pub edges: Vec<Edge>,
    pub labels: Vec<EdgeLabel>,
    /// Members done, failed and given again, and stopped: the narrow module's last line.
    pub done: usize,
    pub failed_again: usize,
    pub stopped: usize,
    pub width: f32,
    pub height: f32,
}

impl People {
    /// The members the narrow module lists: those still in play.
    pub fn active(&self) -> impl Iterator<Item = &MemberNode> {
        self.members.iter().filter(|member| !member.one_line)
    }

    /// The narrow module's last line: "4 done · 1 failed, given again · 2 stopped".
    pub fn rest(&self) -> Option<String> {
        let mut parts = Vec::new();
        if self.done > 0 {
            parts.push(format!("{} done", self.done));
        }
        if self.failed_again > 0 {
            parts.push(format!("{} failed, given again", self.failed_again));
        }
        if self.stopped > 0 {
            parts.push(format!("{} stopped", self.stopped));
        }
        (!parts.is_empty()).then(|| parts.join(" \u{00B7} "))
    }
}

/// The People layer of `team` on the plan `tasks`; `agents` are the chat's agents by
/// thread, whose own events say what a working member does.
pub fn people(team: &Team, tasks: &[Task], agents: &BTreeMap<String, Subagent>) -> People {
    let ended = team.ended().is_some();
    let mut active = Vec::new();
    let mut done = Vec::new();
    let mut failed_again = Vec::new();
    let mut stopped = Vec::new();
    for member in &team.members {
        let found = find_task(tasks, &member.task_id);
        let task = found.map(|(_, task)| task);
        let state = MemberState::of(member, task, agents.get(&member.thread_id), ended);
        let model = member
            .model
            .as_deref()
            .or(task.and_then(|task| task.model.as_deref()));
        let node = MemberNode {
            thread_id: member.thread_id.clone(),
            name: member.name.clone(),
            role: member.role.clone().unwrap_or_else(|| "general".into()),
            model: model.map(short_model),
            task_id: member.task_id.clone(),
            number: found.map(|(number, _)| number),
            task: found.map(|(number, task)| task.numbered(number)),
            spend: task.and_then(Task::spend),
            line: state.line(task, tasks, ended, &member.thread_id),
            word: state.word(task),
            narrow: narrow_line(&state, task, tasks, team),
            state,
            one_line: false,
            rect: Rect::default(),
        };
        let given_again = task
            .and_then(|task| task.owner.as_ref())
            .is_some_and(|owner| owner.thread_id != member.thread_id);
        match &node.state {
            MemberState::Done => done.push(node),
            MemberState::Stopped => stopped.push(node),
            MemberState::Failed if given_again => failed_again.push(node),
            _ => active.push(node),
        }
    }
    // The plan's order, then the name: the same team always draws the same way.
    let order = |node: &MemberNode| (node.number.unwrap_or(usize::MAX), node.name.clone());
    for list in [&mut active, &mut done, &mut failed_again, &mut stopped] {
        list.sort_by_key(order);
    }

    let mut members = Vec::new();
    let mut y = 0.;
    // The top and bottom of the members drawn as nodes, for the lead to stand level with.
    let mut span: Option<(f32, f32)> = None;
    for mut node in active {
        node.rect = Rect {
            x: MEMBERS_X,
            y,
            w: MEMBER_W,
            h: CARD_H,
        };
        span = Some((span.map_or(y, |(top, _)| top), y + CARD_H));
        y += CARD_H + CARD_GAP;
        members.push(node);
    }
    let fold_done = done.len() > DONE_SHOWN || members.len() + done.len() > FOLD_ALL;
    let (done_count, failed_count, stopped_count) = (done.len(), failed_again.len(), stopped.len());
    let mut folds = Vec::new();
    if fold_done {
        let mut by_role: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for node in &done {
            by_role
                .entry(node.role.clone())
                .or_default()
                .push(chip_name(node));
        }
        for (role, names) in by_role {
            let text = match names.len() {
                1 => role,
                count => format!("{role} \u{00D7}{count}"),
            };
            // A long role takes the column's width rather than being cut.
            let w = match text.chars().count() > CHIP_CHARS {
                true => MEMBER_W,
                false => CHIP_W,
            };
            folds.push((TaskStatus::Done, text, names, w));
        }
    } else {
        for mut node in done {
            node.one_line = true;
            node.rect = Rect {
                x: MEMBERS_X,
                y,
                w: MEMBER_W,
                h: LINE_H,
            };
            span = Some((span.map_or(y, |(top, _)| top), y + LINE_H));
            y += LINE_STEP;
            members.push(node);
        }
    }
    if !failed_again.is_empty() {
        let names = failed_again.iter().map(chip_name).collect();
        let text = format!("{} failed, given again", failed_again.len());
        folds.push((TaskStatus::Failed, text, names, MEMBER_W));
    }
    if !stopped.is_empty() {
        let names = stopped.iter().map(chip_name).collect();
        let why = if ended {
            "with the team"
        } else {
            "and given back"
        };
        let text = format!("{} stopped {why}", stopped.len());
        folds.push((TaskStatus::Open, text, names, MEMBER_W));
    }
    let mut x = 0.;
    let mut chips = Vec::new();
    for (status, text, names, w) in folds {
        if x + w > MEMBER_W {
            x = 0.;
            y += CHIP_STEP;
        }
        chips.push(FoldChip {
            status,
            text,
            names,
            rect: Rect {
                x: MEMBERS_X + x,
                y,
                w,
                h: CHIP_H,
            },
        });
        x += w + 6.;
    }
    if !chips.is_empty() {
        y += CHIP_STEP;
    }

    let lead_y = span.map_or(0., |(top, bottom)| {
        ((top + bottom) / 2. - LEAD_H / 2.).max(0.)
    });
    let lead = Rect {
        x: 0.,
        y: lead_y,
        w: LEAD_W,
        h: LEAD_H,
    };
    let mut edges = Vec::new();
    let mut labels = Vec::new();
    for node in &members {
        let (tone, arrow, label) = lead_edge(node, tasks);
        let asks = matches!(node.state, MemberState::Blocked) && arrow == Arrow::In;
        edges.push(Edge::new(
            Ends::Lead(node.thread_id.clone()),
            across(lead, node.rect),
            tone,
            asks,
            arrow,
        ));
        if !label.is_empty() {
            labels.push(EdgeLabel {
                text: label,
                rect: Rect {
                    x: MEMBERS_X - LABEL_W - 8.,
                    y: node.rect.middle_y() - 19.,
                    w: LABEL_W,
                    h: 16.,
                },
                tone,
                right: true,
                ends: Ends::Lead(node.thread_id.clone()),
            });
        }
    }
    waits(&members, tasks, &mut edges, &mut labels);

    let done_tasks = tasks
        .iter()
        .filter(|task| task.status == TaskStatus::Done)
        .count();
    People {
        lead,
        lead_line: lead_line(team, tasks),
        lead_tasks: format!("{done_tasks} of {} tasks done", tasks.len()),
        members,
        folds: chips,
        edges,
        labels,
        done: done_count,
        failed_again: failed_count,
        stopped: stopped_count,
        width: WIDTH,
        height: (y - CARD_GAP).max(lead.y + LEAD_H),
    }
}

fn chip_name(node: &MemberNode) -> String {
    match &node.task {
        Some(task) => format!("{} \u{00B7} {task}", node.name),
        None => node.name.clone(),
    }
}

/// The edge from the lead to `node`: its tone, which way it points, and its words.
fn lead_edge(node: &MemberNode, tasks: &[Task]) -> (EdgeTone, Arrow, String) {
    let number = node.number.map_or_else(String::new, |n| format!(" #{n}"));
    let task = find_task(tasks, &node.task_id).map(|(_, task)| task);
    match &node.state {
        MemberState::Review { .. } => (EdgeTone::Accent, Arrow::In, format!("handed in{number}")),
        MemberState::Blocked => match task.and_then(|task| task.waits_on.as_ref()) {
            Some(WaitsOn::Lead | WaitsOn::Message(_)) => {
                (EdgeTone::Warning, Arrow::In, "asks the lead".into())
            }
            _ => (EdgeTone::Line, Arrow::Out, format!("gave{number}")),
        },
        MemberState::Done | MemberState::Stopped => (EdgeTone::Faint, Arrow::None, String::new()),
        MemberState::Failed => (EdgeTone::Danger, Arrow::None, "failed".into()),
        _ => {
            let reworks = task.map_or(0, |task| task.reworks);
            let label = match reworks {
                0 => format!("gave{number}"),
                1 => format!("sent back{number}"),
                times => format!("sent back{number} \u{00D7}{times}"),
            };
            (EdgeTone::Line, Arrow::Out, label)
        }
    }
}

/// A dashed arc from each member that waits on another's task to that member.
fn waits(
    members: &[MemberNode],
    tasks: &[Task],
    edges: &mut Vec<Edge>,
    labels: &mut Vec<EdgeLabel>,
) {
    let full: Vec<&MemberNode> = members.iter().filter(|node| !node.one_line).collect();
    let by_thread: HashMap<&str, &MemberNode> = full
        .iter()
        .map(|node| (node.thread_id.as_str(), *node))
        .collect();
    for node in full {
        let Some(task) = find_task(tasks, &node.task_id).map(|(_, task)| task) else {
            continue;
        };
        let Some(WaitsOn::Task(on)) = &task.waits_on else {
            continue;
        };
        let Some((number, on_task)) = find_task(tasks, on) else {
            continue;
        };
        let Some(other) = on_task
            .owner
            .as_ref()
            .and_then(|owner| by_thread.get(owner.thread_id.as_str()))
            .filter(|other| other.thread_id != node.thread_id)
        else {
            continue;
        };
        let apart = (other.rect.middle_y() - node.rect.middle_y()).abs();
        let reach = 26. + (apart * 0.2).min(52.);
        let ends = Ends::Members(node.thread_id.clone(), other.thread_id.clone());
        edges.push(Edge::new(
            ends.clone(),
            arc(node.rect, other.rect, reach),
            EdgeTone::Warning,
            true,
            Arrow::Out,
        ));
        labels.push(EdgeLabel {
            text: format!("waits on #{number} \u{00B7} {}", other.name),
            rect: Rect {
                x: node.rect.right() + reach * 1.33 + 8.,
                y: (node.rect.middle_y() + other.rect.middle_y()) / 2. - 8.,
                w: WAIT_LABEL_W,
                h: 16.,
            },
            tone: EdgeTone::Warning,
            right: false,
            ends,
        });
    }
}

/// The lead's line: how many are at work, in review and blocked, or how the team ended.
fn lead_line(team: &Team, tasks: &[Task]) -> String {
    match team.ended() {
        Some(TeamEnd::Done) => return "done".into(),
        Some(TeamEnd::Stopped) => return "stopped".into(),
        Some(end) => return format!("ended \u{2014} {}", end.reason()),
        None => {}
    }
    let count = |status| tasks.iter().filter(|task| task.status == status).count();
    let mut parts = vec![format!("{} at work", team.at_work())];
    for (status, word) in [
        (TaskStatus::Review, "in review"),
        (TaskStatus::Blocked, "blocked"),
    ] {
        match count(status) {
            0 => {}
            n => parts.push(format!("{n} {word}")),
        }
    }
    parts.join(" \u{00B7} ")
}

/// The narrow row's second line: what a blocked member waits on, what a working one
/// does, why a failed one failed, or what it spent.
fn narrow_line(state: &MemberState, task: Option<&Task>, tasks: &[Task], team: &Team) -> String {
    let note = task
        .map(|task| task.note.trim().to_owned())
        .unwrap_or_default();
    match state {
        MemberState::Blocked => {
            let on = task.and_then(|task| match &task.waits_on {
                Some(WaitsOn::Task(id)) => find_task(tasks, id),
                _ => None,
            });
            let owner = on.and_then(|(_, task)| task.owner.as_ref());
            match (on, owner) {
                (Some((number, _)), Some(owner))
                    if team.members.iter().any(|m| m.thread_id == owner.thread_id) =>
                {
                    format!("waits on {} \u{00B7} #{number}", owner.name)
                }
                _ if !note.is_empty() => note,
                _ => "blocked".into(),
            }
        }
        MemberState::Failed if !note.is_empty() => note,
        MemberState::Tool(_) | MemberState::Given => state.line(task, tasks, false, ""),
        _ => task
            .and_then(Task::spend)
            .unwrap_or_else(|| plural(0, "step")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::thread::{TaskOwner, TeamBudget, TeamEnd, TeamMember, TeamStatus, TeamUsed};

    fn task(number: usize, status: TaskStatus, owner: Option<&str>) -> Task {
        Task {
            task_id: format!("t{number}"),
            description: format!("task {number}"),
            status,
            done: status == TaskStatus::Done,
            owner: owner.map(|owner| TaskOwner {
                thread_id: owner.into(),
                name: owner.to_uppercase(),
            }),
            ..Task::default()
        }
    }

    fn team(status: TeamStatus, members: &[(&str, usize, TaskStatus)]) -> Team {
        Team {
            thread_id: "lead".into(),
            started_at: None,
            ended_at: None,
            status,
            budget: TeamBudget::new(8, 400, 4_000_000, 120).unwrap(),
            used: TeamUsed::default(),
            members: members
                .iter()
                .map(|(thread, number, status)| TeamMember {
                    thread_id: (*thread).into(),
                    name: thread.to_uppercase(),
                    role: Some(if number % 2 == 0 { "qa" } else { "backend" }.into()),
                    model: Some("chatgpt/gpt-5.5".into()),
                    task_id: format!("t{number}"),
                    status: *status,
                })
                .collect(),
        }
    }

    /// `count` members, each with a done task of its own.
    fn done_team(count: usize) -> (Team, Vec<Task>) {
        let threads: Vec<String> = (1..=count).map(|n| format!("m{n}")).collect();
        let tasks = threads
            .iter()
            .enumerate()
            .map(|(k, thread)| task(k + 1, TaskStatus::Done, Some(thread)))
            .collect();
        let members: Vec<(&str, usize, TaskStatus)> = threads
            .iter()
            .enumerate()
            .map(|(k, thread)| (thread.as_str(), k + 1, TaskStatus::Done))
            .collect();
        (team(TeamStatus::Running, &members), tasks)
    }

    #[test]
    fn two_members_stand_beside_the_lead_with_their_edges() {
        let mut tasks = vec![
            task(1, TaskStatus::Running, Some("a")),
            task(2, TaskStatus::Blocked, Some("b")),
        ];
        tasks[1].waits_on = Some(WaitsOn::Task("t1".into()));
        let team = team(
            TeamStatus::Running,
            &[("b", 2, TaskStatus::Blocked), ("a", 1, TaskStatus::Running)],
        );
        let layer = people(&team, &tasks, &BTreeMap::new());
        let names: Vec<&str> = layer.members.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(
            names,
            ["A", "B"],
            "in plan order, not the team's newest first"
        );
        assert!(layer.members.iter().all(|m| m.rect.x == MEMBERS_X));
        // The lead stands level with the middle of the column.
        let bottom = layer.members[1].rect.y + layer.members[1].rect.h;
        assert_eq!(
            layer.lead.middle_y(),
            (layer.members[0].rect.y + bottom) / 2.
        );
        // One edge each from the lead, and B's arc to A.
        assert_eq!(layer.edges.len(), 3);
        let wait = &layer.edges[2];
        assert_eq!(wait.ends, Ends::Members("b".into(), "a".into()));
        assert!(wait.dashed && wait.arrow.is_some());
        let words: Vec<&str> = layer.labels.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(words, ["gave #1", "gave #2", "waits on #1 \u{00B7} A"]);
        assert_eq!(layer.members[1].narrow, "waits on A \u{00B7} #1");
        assert_eq!(layer.lead_line, "1 at work \u{00B7} 1 blocked");
    }

    #[test]
    fn done_members_shrink_to_a_line_then_fold_by_role() {
        let (few, tasks) = done_team(3);
        let layer = people(&few, &tasks, &BTreeMap::new());
        assert!(
            layer
                .members
                .iter()
                .all(|m| m.one_line && m.rect.h == LINE_H)
        );
        assert!(layer.folds.is_empty());

        let (many, tasks) = done_team(7);
        let layer = people(&many, &tasks, &BTreeMap::new());
        assert!(layer.members.is_empty());
        let chips: Vec<&str> = layer.folds.iter().map(|f| f.text.as_str()).collect();
        assert_eq!(chips, ["backend \u{00D7}4", "qa \u{00D7}3"]);
        assert_eq!(layer.rest().as_deref(), Some("7 done"));
    }

    #[test]
    fn an_ended_team_stops_its_members_and_says_why() {
        let tasks = vec![
            task(1, TaskStatus::Done, Some("a")),
            task(2, TaskStatus::Open, None),
        ];
        let team = team(
            TeamStatus::Ended(TeamEnd::Steps),
            &[("a", 1, TaskStatus::Done), ("b", 2, TaskStatus::Running)],
        );
        let layer = people(&team, &tasks, &BTreeMap::new());
        assert_eq!(layer.stopped, 1);
        assert_eq!(layer.folds[0].text, "1 stopped with the team");
        assert!(
            layer
                .lead_line
                .starts_with("ended \u{2014} ran out of steps")
        );
        assert_eq!(layer.lead_tasks, "1 of 2 tasks done");
        assert_eq!(layer.active().count(), 0);
    }
}
