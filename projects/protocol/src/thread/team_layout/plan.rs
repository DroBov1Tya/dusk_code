//! The Plan layer in waves: a task's wave is one past the deepest of the tasks it needs,
//! so it stands right of all of them. Within a wave a task stands near the middle of what
//! it needs. Past `FOLD_PAST` tasks the waves already done fold into one column.

use std::collections::HashMap;

use super::{Arrow, Edge, EdgeTone, Ends, Rect, across, arc};
use crate::thread::{GoalCheck, Task, TaskDuty, TaskStatus, WaitsOn};

/// A plan of more tasks than this folds its done waves.
pub const FOLD_PAST: usize = 40;
const NODE_W: f32 = 170.;
const NODE_H: f32 = 58.;
const ROW_GAP: f32 = 8.;
const WAVE_GAP: f32 = 26.;
/// The wave heads' line above the nodes.
const HEAD_H: f32 = 24.;
const FOLD_W: f32 = 54.;

/// The head of a wave shown: "wave 3", "2 of 5 done".
#[derive(Debug, Clone, PartialEq)]
pub struct WaveHead {
    pub name: String,
    pub done: String,
    pub rect: Rect,
}

/// A task as a node of the layer.
#[derive(Debug, Clone, PartialEq)]
pub struct TaskNode {
    pub task_id: String,
    pub number: usize,
    pub title: String,
    pub status: TaskStatus,
    /// "in work", "asks the lead", "confirm".
    pub word: &'static str,
    /// Its review waits for the user: the word is a button that confirms it.
    pub for_user: bool,
    /// The member that holds it or did, by thread and name.
    pub owner: Option<(String, String)>,
    pub role: String,
    pub reworks: u32,
    pub attempts: u32,
    pub gated: bool,
    /// Why it is blocked or failed; model text.
    pub note: String,
    /// Its wave, from 0.
    pub wave: usize,
    /// How its fresh review stands (`Task::review_word`); reviews are no nodes.
    pub review: Option<String>,
    /// The lead gave it up.
    pub skipped: bool,
    /// The core's completeness check at the end.
    pub critic: bool,
    pub rect: Rect,
}

/// The done waves folded into one column.
#[derive(Debug, Clone, PartialEq)]
pub struct Folded {
    pub waves: usize,
    pub tasks: usize,
    /// "waves 1–3".
    pub name: String,
    pub rect: Rect,
}

/// The Plan layer.
#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    pub heads: Vec<WaveHead>,
    pub tasks: Vec<TaskNode>,
    pub folded: Option<Folded>,
    /// Done waves can fold: the plan is past `FOLD_PAST` and has some.
    pub foldable: bool,
    pub edges: Vec<Edge>,
    pub width: f32,
    pub height: f32,
}

/// Each task's wave, from 0, in the order of `tasks`. A task needs those of its
/// `depends_on` that are in the plan; one that needs itself or closes a cycle has that
/// need left out, so every task gets a wave. The critic stands one wave past the rest.
pub fn depths(tasks: &[Task]) -> Vec<usize> {
    let nodes: Vec<(&str, &[String], TaskDuty)> = tasks
        .iter()
        .map(|task| (task.task_id.as_str(), task.depends_on.as_slice(), task.duty))
        .collect();
    depths_of(&nodes)
}

/// `depths` for the core, which holds tasks of its own: each node is its id, its
/// `depends_on` and its duty. The one rule of waves, so the team's barrier and every
/// client draw the same waves.
pub fn depths_of(nodes: &[(&str, &[String], TaskDuty)]) -> Vec<usize> {
    let index: HashMap<&str, usize> = nodes
        .iter()
        .enumerate()
        .map(|(at, (id, _, _))| (*id, at))
        .collect();
    let needs: Vec<Vec<usize>> = nodes
        .iter()
        .enumerate()
        .map(|(at, (_, depends_on, _))| {
            depends_on
                .iter()
                .filter_map(|id| index.get(id.as_str()).copied())
                .filter(|&need| need != at)
                .collect()
        })
        .collect();
    let mut depth: Vec<Option<usize>> = vec![None; nodes.len()];
    let mut on_path = vec![false; nodes.len()];
    for start in 0..nodes.len() {
        if depth[start].is_some() {
            continue;
        }
        // Depth first without recursion: a long chain cannot run out of stack.
        let mut path = vec![(start, 0)];
        on_path[start] = true;
        while let Some((at, next)) = path.last_mut() {
            let at = *at;
            if let Some(&need) = needs[at].get(*next) {
                *next += 1;
                if depth[need].is_none() && !on_path[need] {
                    on_path[need] = true;
                    path.push((need, 0));
                }
            } else {
                let deepest = needs[at].iter().filter_map(|&need| depth[need]).max();
                depth[at] = Some(deepest.map_or(0, |deepest| deepest + 1));
                on_path[at] = false;
                path.pop();
            }
        }
    }
    let mut depths: Vec<usize> = depth.into_iter().map(Option::unwrap_or_default).collect();
    let is_critic = |at: usize| nodes[at].2 == TaskDuty::Critic;
    let last = (0..nodes.len())
        .filter(|&at| !is_critic(at))
        .map(|at| depths[at] + 1)
        .max()
        .unwrap_or(0);
    for (at, depth) in depths.iter_mut().enumerate() {
        if is_critic(at) {
            *depth = last;
        }
    }
    depths
}

/// The plan's tasks by wave, as their indexes in `tasks`, in the plan's order. A review
/// is no task of its own here: it rides on the task it reviews (`Task::review_word`).
pub fn waves(tasks: &[Task]) -> Vec<Vec<usize>> {
    let depths = depths(tasks);
    let mut waves: Vec<Vec<usize>> = Vec::new();
    for (at, depth) in depths.into_iter().enumerate() {
        if tasks[at].duty == TaskDuty::Review {
            continue;
        }
        if waves.len() <= depth {
            waves.resize_with(depth + 1, Vec::new);
        }
        waves[depth].push(at);
    }
    waves
}

/// A task's word in the Plan layer.
pub fn task_word(task: &Task) -> &'static str {
    match task.status {
        TaskStatus::Open => "queued",
        TaskStatus::Claimed => "given",
        TaskStatus::Running if task.reworks > 0 => "rework",
        TaskStatus::Running => "in work",
        TaskStatus::Blocked => match task.waits_on {
            Some(WaitsOn::Lead) => "asks the lead",
            _ => "blocked",
        },
        TaskStatus::Review if task.gate == Some(GoalCheck::UserConfirmation) => "confirm",
        TaskStatus::Review => "in review",
        TaskStatus::Done => "done",
        TaskStatus::Failed => "failed",
    }
}

/// The Plan layer of `tasks`; `unfold` shows the done waves a long plan folds.
pub fn plan(tasks: &[Task], unfold: bool) -> Plan {
    let waves = waves(tasks);
    let wave_of: Vec<usize> = {
        let mut of = vec![0; tasks.len()];
        for (wave, members) in waves.iter().enumerate() {
            for &at in members {
                of[at] = wave;
            }
        }
        of
    };
    let all_done = |wave: &Vec<usize>| wave.iter().all(|&at| tasks[at].finished());
    // The first wave with work left; one done wave stays in sight before it.
    let first_open = waves
        .iter()
        .position(|wave| !all_done(wave))
        .unwrap_or(waves.len());
    let can_fold = first_open.saturating_sub(1);
    let foldable = tasks.len() > FOLD_PAST && can_fold > 0;
    let fold = if foldable && !unfold { can_fold } else { 0 };

    let mut x = 0.;
    let folded = (fold > 0).then(|| {
        x = FOLD_W + WAVE_GAP;
        Folded {
            waves: fold,
            tasks: waves[..fold].iter().map(Vec::len).sum(),
            name: match fold {
                1 => "wave 1".into(),
                _ => format!("waves 1\u{2013}{fold}"),
            },
            rect: Rect::default(),
        }
    });
    let rows = waves[fold..].iter().map(Vec::len).max().unwrap_or(0);
    let height = HEAD_H + rows as f32 * (NODE_H + ROW_GAP);
    let mut heads = Vec::new();
    let mut place: Vec<Option<Rect>> = vec![None; tasks.len()];
    let mut nodes = Vec::new();
    for (wave, members) in waves.iter().enumerate().skip(fold) {
        let done = members
            .iter()
            .filter(|&&at| tasks[at].status == TaskStatus::Done)
            .count();
        let check = members.iter().all(|&at| tasks[at].duty == TaskDuty::Critic);
        heads.push(WaveHead {
            name: match check && !members.is_empty() {
                true => "check".into(),
                false => format!("wave {}", wave + 1),
            },
            done: format!("{done} of {} done", members.len()),
            rect: Rect {
                x,
                y: 0.,
                w: NODE_W,
                h: 16.,
            },
        });
        // Near the middle of what it needs; a task that needs nothing shown keeps its
        // place in the plan.
        let mut order: Vec<(f32, usize)> = members
            .iter()
            .enumerate()
            .map(|(k, &at)| {
                let ys: Vec<f32> = needs_of(tasks, at)
                    .filter_map(|need| place[need].map(|rect| rect.y))
                    .collect();
                let middle = match ys.is_empty() {
                    true => k as f32 * 1_000.,
                    false => ys.iter().sum::<f32>() / ys.len() as f32,
                };
                (middle, at)
            })
            .collect();
        order.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        let offset = (rows - members.len()) as f32 * (NODE_H + ROW_GAP) / 2.;
        for (k, (_, at)) in order.into_iter().enumerate() {
            let rect = Rect {
                x,
                y: HEAD_H + offset + k as f32 * (NODE_H + ROW_GAP),
                w: NODE_W,
                h: NODE_H,
            };
            place[at] = Some(rect);
            nodes.push(node(tasks, at, wave_of[at], rect));
        }
        x += NODE_W + WAVE_GAP;
    }
    let folded = folded.map(|folded| Folded {
        rect: Rect {
            x: 0.,
            y: HEAD_H,
            w: FOLD_W,
            h: (height - HEAD_H - 2.).max(NODE_H),
        },
        ..folded
    });

    let mut edges = Vec::new();
    for node in &nodes {
        let at = node.number - 1;
        let to = node.rect;
        for need in needs_of(tasks, at) {
            let from = match (place[need], &folded) {
                (Some(rect), _) => rect,
                (None, Some(folded)) => folded.rect,
                (None, None) => continue,
            };
            let tone = match tasks[need].status {
                TaskStatus::Done => EdgeTone::Faint,
                _ => EdgeTone::Line,
            };
            edges.push(Edge::new(
                Ends::Tasks(tasks[need].task_id.clone(), node.task_id.clone()),
                across(from, to),
                tone,
                false,
                Arrow::None,
            ));
        }
        // Waiting on a task it does not need: a dashed arc between the two.
        if let Some(WaitsOn::Task(on)) = &tasks[at].waits_on {
            let needed = tasks[at].depends_on.iter().any(|id| id == on);
            let other = tasks.iter().position(|task| task.task_id == *on);
            if let (false, Some(other)) = (needed, other)
                && let Some(rect) = place[other]
            {
                edges.push(Edge::new(
                    Ends::Tasks(on.clone(), node.task_id.clone()),
                    arc(to, rect, 16.),
                    EdgeTone::Warning,
                    true,
                    Arrow::Out,
                ));
            }
        }
    }
    let (width, height) = match nodes.is_empty() {
        true => (0., 0.),
        false => (x - WAVE_GAP, height),
    };
    Plan {
        heads,
        tasks: nodes,
        folded,
        foldable,
        edges,
        width,
        height,
    }
}

/// The indexes of the tasks the task at `at` needs, in the plan.
fn needs_of(tasks: &[Task], at: usize) -> impl Iterator<Item = usize> + '_ {
    tasks[at].depends_on.iter().filter_map(move |id| {
        tasks
            .iter()
            .position(|task| task.task_id == *id)
            .filter(|&need| need != at)
    })
}

fn node(tasks: &[Task], at: usize, wave: usize, rect: Rect) -> TaskNode {
    let task = &tasks[at];
    TaskNode {
        task_id: task.task_id.clone(),
        number: at + 1,
        title: task.title(),
        status: task.status,
        word: task_word(task),
        for_user: task.status == TaskStatus::Review
            && task.gate == Some(GoalCheck::UserConfirmation),
        owner: task
            .owner
            .as_ref()
            .map(|owner| (owner.thread_id.clone(), owner.name.clone())),
        role: task.role.clone().unwrap_or_else(|| "general".into()),
        reworks: task.reworks,
        attempts: task.attempts,
        gated: task.gate.is_some(),
        note: task.note.trim().to_owned(),
        wave,
        review: task.review_word(tasks),
        skipped: task.skipped,
        critic: task.duty == TaskDuty::Critic,
        rect,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task(id: &str, needs: &[&str], status: TaskStatus) -> Task {
        Task {
            task_id: id.into(),
            description: format!("do {id}"),
            status,
            done: status == TaskStatus::Done,
            depends_on: needs.iter().map(|need| (*need).into()).collect(),
            ..Task::default()
        }
    }

    #[test]
    fn a_task_stands_one_wave_past_the_deepest_it_needs() {
        let tasks = [
            task("a", &[], TaskStatus::Done),
            task("b", &["a"], TaskStatus::Done),
            task("c", &["b"], TaskStatus::Running),
            task("d", &["b"], TaskStatus::Blocked),
            task("e", &["a"], TaskStatus::Open),
            task("f", &["c", "d"], TaskStatus::Open),
            task("g", &["f", "a"], TaskStatus::Open),
        ];
        assert_eq!(depths(&tasks), [0, 1, 2, 2, 1, 3, 4]);
        let layer = plan(&tasks, false);
        for node in &layer.tasks {
            for need in &tasks[node.number - 1].depends_on {
                let needed = layer.tasks.iter().find(|n| n.task_id == *need).unwrap();
                assert!(
                    needed.rect.right() < node.rect.x,
                    "{need} before {}",
                    node.task_id
                );
            }
        }
        assert_eq!(layer.heads.len(), 5);
        assert_eq!(layer.heads[1].done, "1 of 2 done");
        assert_eq!(layer.edges.len(), 8);
        assert!(!layer.foldable && layer.folded.is_none());
    }

    #[test]
    fn cycles_self_needs_and_missing_tasks_never_panic() {
        let tasks = [
            task("a", &["c"], TaskStatus::Open),
            task("b", &["a"], TaskStatus::Open),
            task("c", &["b"], TaskStatus::Open),
            task("d", &["d", "gone"], TaskStatus::Open),
        ];
        let found = depths(&tasks);
        assert_eq!(
            found[3], 0,
            "a task needing only itself or nothing known is first"
        );
        // The cycle is cut where it closes; every task has a wave.
        assert_eq!(found[..3], [2, 0, 1]);
        let layer = plan(&tasks, false);
        assert_eq!(layer.tasks.len(), 4);
        assert!(plan(&[], false).tasks.is_empty());
        // A long chain needs no deep stack.
        let chain: Vec<Task> = (0..5_000)
            .map(|n| {
                let need = format!("t{}", n + 1);
                task(&format!("t{n}"), &[need.as_str()], TaskStatus::Open)
            })
            .collect();
        assert_eq!(depths(&chain)[0], 4_999);
    }

    /// Nine waves of five: the first `done` waves done.
    fn long(done: usize) -> Vec<Task> {
        let mut tasks = Vec::new();
        for wave in 0..9 {
            for k in 0..5 {
                let needs = match wave {
                    0 => Vec::new(),
                    _ => vec![format!("w{}k{k}", wave - 1)],
                };
                let needs: Vec<&str> = needs.iter().map(String::as_str).collect();
                let status = match wave < done {
                    true => TaskStatus::Done,
                    false => TaskStatus::Open,
                };
                tasks.push(task(&format!("w{wave}k{k}"), &needs, status));
            }
        }
        tasks
    }

    #[test]
    fn past_forty_tasks_the_done_waves_fold() {
        let tasks = long(4);
        assert_eq!(tasks.len(), 45);
        let layer = plan(&tasks, false);
        let folded = layer.folded.as_ref().unwrap();
        // Four done waves: three fold, the last stays in sight before the open one.
        assert_eq!((folded.waves, folded.tasks), (3, 15));
        assert_eq!(folded.name, "waves 1\u{2013}3");
        assert_eq!(layer.heads[0].name, "wave 4");
        assert_eq!(layer.tasks.len(), 30);
        assert!(layer.foldable);
        // A task of the first wave shown needs a folded one: its edge comes from the fold.
        let edge = layer
            .edges
            .iter()
            .find(|e| e.ends == Ends::Tasks("w2k0".into(), "w3k0".into()));
        assert_eq!(edge.unwrap().curve[0].x, folded.rect.right() + 2.);
        // Unfolded, everything shows.
        let open = plan(&tasks, true);
        assert!(open.folded.is_none() && open.foldable);
        assert_eq!(open.tasks.len(), 45);
        // Forty tasks never fold.
        assert!(plan(&tasks[..40], false).folded.is_none());
        // Nothing done, nothing folds.
        assert!(plan(&long(0), false).folded.is_none());
    }

    #[test]
    fn the_same_plan_lays_out_the_same_way() {
        let tasks = long(2);
        let once = plan(&tasks, false);
        assert_eq!(once, plan(&tasks, false));
        let ids: Vec<&str> = once
            .tasks
            .iter()
            .take(5)
            .map(|n| n.task_id.as_str())
            .collect();
        // Wave 1 folded; wave 2 in the plan's order, as each needs the one before it.
        assert_eq!(ids, ["w1k0", "w1k1", "w1k2", "w1k3", "w1k4"]);
    }
}

#[cfg(test)]
mod checks {
    use super::*;

    #[test]
    fn a_review_rides_on_its_task_and_the_critic_is_the_last_column() {
        let task = |id: &str, needs: &[&str], duty: TaskDuty| Task {
            task_id: id.into(),
            description: format!("do {id}"),
            depends_on: needs.iter().map(|need| (*need).into()).collect(),
            duty,
            ..Task::default()
        };
        let mut review = task("r", &["a"], TaskDuty::Review);
        review.review_of = Some("b".into());
        review.status = TaskStatus::Done;
        review.done = true;
        review.result = "Problem: high | x | y | z\nVerdict: fix".into();
        let tasks = [
            task("a", &[], TaskDuty::Work),
            task("b", &["a"], TaskDuty::Work),
            review,
            task("c", &[], TaskDuty::Critic),
        ];
        assert_eq!(depths(&tasks), [0, 1, 1, 2]);
        assert_eq!(waves(&tasks), [vec![0], vec![1], vec![3]]);
        let layout = plan(&tasks, false);
        assert_eq!(layout.tasks.len(), 3, "the review is no node");
        assert_eq!(layout.heads[2].name, "check");
        let b = layout
            .tasks
            .iter()
            .find(|node| node.task_id == "b")
            .unwrap();
        assert_eq!(b.review.as_deref(), Some("review: fix"));
    }
}
