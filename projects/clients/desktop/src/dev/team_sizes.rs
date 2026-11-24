//! `fake=team-2|team-6|team-24|team-24-ended|team-empty`: after `fake=chat`, the shown
//! chat leads a team of that many members, for the Team page at each size: two members
//! on a plan of four, six on eight, and twenty-four on a plan of forty-four whose first
//! waves are done (so the done members fold by role and the done waves fold); `-ended` is
//! the large one after it ran out of steps, `team-empty` a TEAM chat with no plan yet.
//! As the core tells it: `TaskChanged` for every task, `SubagentStarted` and the
//! members' own events, then `Team`. Made-up data only.

use dusk_protocol::{
    model::{ModelEvent, ToolCall},
    system::now_ms,
    thread::{
        Task, TaskStatus, Team, TeamEnd, TeamMember, TeamStatus, TeamUsed, ThreadEvent, agent_name,
        team_call_id,
    },
};
use gpui_kit::*;
use serde_json::{Value, json};

use crate::workspace::Workspace;

const ROLES: [&str; 6] = [
    "backend-engineer",
    "backend-engineer",
    "qa-engineer",
    "code-reviewer",
    "sre-engineer",
    "docs",
];
const VERBS: [&str; 8] = [
    "Add", "Move", "Test", "Review", "Harden", "Split", "Wire", "Measure",
];
const THINGS: [&str; 10] = [
    "the eviction policy",
    "the TTL index",
    "the write path",
    "the read path",
    "cache metrics",
    "the config loader",
    "the LRU list",
    "the snapshot format",
    "key hashing",
    "the size limit",
];

/// A task of the made-up plan: its words, status, role, what it needs, whether a member
/// holds it, and its other fields.
struct Made {
    text: String,
    status: TaskStatus,
    role: &'static str,
    needs: Vec<usize>,
    owned: bool,
    extra: Value,
}

fn made(text: &str, status: TaskStatus, role: &'static str, needs: &[usize], extra: Value) -> Made {
    Made {
        text: text.into(),
        status,
        role,
        needs: needs.to_vec(),
        owned: status != TaskStatus::Open,
        extra,
    }
}

fn small(members: usize) -> Vec<Made> {
    use TaskStatus::*;
    match members {
        2 => vec![
            made(
                "Give Entry a deadline",
                Running,
                "backend-engineer",
                &[],
                json!({ "steps": 14, "tokens": 72_000, "gate": { "kind": "command", "command": "cargo test -p cache" } }),
            ),
            made(
                "Write tests for expiry",
                Blocked,
                "qa-engineer",
                &[],
                json!({ "waits_on": { "task": "t1" }, "note": "needs the deadline field first", "steps": 6, "tokens": 31_000 }),
            ),
            made("Review the diff", Open, "code-reviewer", &[1, 2], json!({})),
            made("Write what changed", Open, "docs", &[3], json!({})),
        ],
        _ => vec![
            made(
                "Plan the cache layout",
                Done,
                "architect",
                &[],
                json!({ "steps": 14, "tokens": 61_000 }),
            ),
            made(
                "Give Entry a deadline",
                Done,
                "backend-engineer",
                &[1],
                json!({ "steps": 22, "tokens": 118_000 }),
            ),
            made(
                "Remove expired entries in get()",
                Running,
                "backend-engineer",
                &[2],
                json!({ "reworks": 1, "attempts": 2, "steps": 31, "tokens": 204_000,
                        "gate": { "kind": "command", "command": "cargo test -p cache" } }),
            ),
            made(
                "Write tests for expiry",
                Blocked,
                "qa-engineer",
                &[2],
                json!({ "waits_on": { "task": "t3" }, "note": "needs get() to drop entries first",
                        "steps": 12, "tokens": 84_000 }),
            ),
            made(
                "Check the cache key handling",
                Review,
                "code-reviewer",
                &[1],
                json!({ "gate": { "kind": "user_confirmation" }, "steps": 9, "tokens": 52_000 }),
            ),
            made(
                "Measure memory growth",
                Claimed,
                "sre-engineer",
                &[2],
                json!({}),
            ),
            made(
                "Review the diff of src/cache.rs",
                Open,
                "code-reviewer",
                &[3, 4],
                json!({}),
            ),
            made("Write what changed", Open, "docs", &[5, 6, 7], json!({})),
        ],
    }
}

/// Forty-four tasks in eight waves; the first four done, the fifth at work.
fn large() -> Vec<Made> {
    use TaskStatus::*;
    const WIDTHS: [usize; 8] = [1, 3, 6, 8, 8, 7, 6, 5];
    let mut plan = Vec::new();
    let mut previous: Vec<usize> = Vec::new();
    for (wave, &width) in WIDTHS.iter().enumerate() {
        let mut current = Vec::new();
        for k in 0..width {
            let number = plan.len() + 1;
            let mut needs = Vec::new();
            if !previous.is_empty() {
                needs.push(previous[k % previous.len()]);
                if k % 3 == 1 {
                    let other = previous[(k + 1) % previous.len()];
                    if !needs.contains(&other) {
                        needs.push(other);
                    }
                }
            }
            let status = match (wave, k) {
                (0..=3, _) => Done,
                (4, 0..=2) => Running,
                (4, 3) => Blocked,
                (4, 4) => Review,
                (4, 5) => Failed,
                (4, 6) => Claimed,
                (4, _) => Running,
                _ => Open,
            };
            let text = match number {
                1 => "Plan the cache rework".to_owned(),
                _ => format!(
                    "{} {}",
                    VERBS[number % VERBS.len()],
                    THINGS[(number * 3 + wave) % THINGS.len()]
                ),
            };
            let role = if wave == 0 {
                "architect"
            } else {
                ROLES[(number * 7) % ROLES.len()]
            };
            let spend = json!({ "steps": 6 + number % 23, "tokens": 18_000 + number * 4_100 });
            let extra = match status {
                Blocked => json!({ "waits_on": { "task": format!("t{}", number - 2) },
                                   "note": format!("needs #{} merged first", number - 2),
                                   "steps": 9, "tokens": 40_000 }),
                Review => {
                    json!({ "gate": { "kind": "user_confirmation" }, "steps": 11, "tokens": 51_000 })
                }
                Failed => {
                    json!({ "attempts": 2, "note": "gate failed twice: unresolved import in cargo build",
                                  "steps": 30, "tokens": 190_000 })
                }
                _ => spend,
            };
            plan.push(Made {
                text,
                status,
                role,
                needs,
                owned: status != Open,
                extra,
            });
            current.push(number);
        }
        previous = current;
    }
    plan
}

/// The thread of the member that holds task `number`; its name comes from it.
fn member_thread(number: usize) -> String {
    format!(
        "0190f1c2-7a3b-7c4d-8e5f-{:012x}",
        (number as u64 + 17) * 0x9e37_79b9
    )
}

fn task(number: usize, made: &Made, now: i64) -> Task {
    let mut task = json!({
        "task_id": format!("t{number}"), "description": made.text,
        "done": made.status == TaskStatus::Done, "status": made.status.name(),
        "created_at": now - 3_400_000, "updated_at": now - 60_000,
        "items": [], "role": made.role, "phase": "cache",
        "attempts": u32::from(made.owned),
        "depends_on": made.needs.iter().map(|need| format!("t{need}")).collect::<Vec<_>>(),
        "model": made.owned.then_some("chatgpt/gpt-5.5"),
    });
    if made.owned {
        let thread_id = member_thread(number);
        task["owner"] = json!({ "name": agent_name(&thread_id), "thread_id": thread_id });
    }
    if let (Some(task), Some(extra)) = (task.as_object_mut(), made.extra.as_object()) {
        task.extend(extra.clone());
    }
    serde_json::from_value(task).expect("a made-up task reads back")
}

/// What a member does now, as its own events say.
fn member_events(number: usize, status: TaskStatus) -> Vec<ThreadEvent> {
    let tool = |name: &str, arguments: Value| {
        ThreadEvent::ToolStarted(ToolCall {
            id: format!("call-{number}-{name}"),
            name: name.into(),
            arguments,
        })
    };
    let started = ThreadEvent::Model(ModelEvent::Text("Reading the task.".into()));
    match status {
        // Some think, some run a command, some read: a running team is never all alike.
        TaskStatus::Running => match number % 3 {
            0 => vec![],
            1 => vec![
                started,
                tool("run_command", json!({ "command": "cargo test -p cache" })),
            ],
            _ => vec![
                started,
                tool("read_file", json!({ "path": "src/cache.rs" })),
            ],
        },
        TaskStatus::Claimed => vec![],
        TaskStatus::Failed => vec![ThreadEvent::Failed("the gate failed twice".into())],
        _ => vec![started, ThreadEvent::Idle],
    }
}

pub fn fake(ws: &mut Workspace, value: &str, cx: &mut Context<Workspace>) {
    let now = now_ms();
    let Some(lead) = ws.shown.clone() else {
        eprintln!("fake={value} needs a shown chat: fake=chat first");
        return;
    };
    let ended = value.ends_with("-ended");
    let mut plan = match value.trim_end_matches("-ended") {
        "team-empty" => Vec::new(),
        "team-2" => small(2),
        "team-6" => small(6),
        _ => large(),
    };
    // The end gave the tasks at work back: they are open, and their members stopped.
    let mut stopped = Vec::new();
    if ended {
        for (index, made) in plan.iter_mut().enumerate() {
            if made.status.at_work() {
                made.status = TaskStatus::Open;
                made.owned = false;
                stopped.push(index + 1);
            }
        }
    }
    let tasks: Vec<Task> = plan
        .iter()
        .enumerate()
        .map(|(index, made)| task(index + 1, made, now))
        .collect();
    let owners: Vec<(usize, &Made)> = plan
        .iter()
        .enumerate()
        .map(|(index, made)| (index + 1, made))
        .filter(|(number, made)| made.owned || stopped.contains(number))
        .collect();
    let members: Vec<TeamMember> = owners
        .iter()
        .rev()
        .map(|(number, made)| {
            let thread_id = member_thread(*number);
            TeamMember {
                name: agent_name(&thread_id),
                thread_id,
                role: Some(made.role.into()),
                model: Some("chatgpt/gpt-5.5".into()),
                task_id: format!("t{number}"),
                // A member stopped by the end still says what its task was.
                status: match stopped.contains(number) {
                    true => TaskStatus::Running,
                    false => made.status,
                },
            }
        })
        .collect();
    let at_work = members.iter().filter(|m| m.status.at_work()).count() as u32;
    let team = Team {
        thread_id: lead.clone(),
        started_at: None,
        winding_since: None,
        goal: String::new(),
        status: match ended {
            true => TeamStatus::Ended(TeamEnd::Stalled),
            false => TeamStatus::Running,
        },
        at_once: 8,
        used: TeamUsed {
            at_once: if ended { 0 } else { at_work },
            steps: if ended {
                600
            } else {
                14 * members.len() as u32
            },
            tokens: 160_000 * members.len() as u64,
            minutes: if ended {
                74
            } else {
                4 + 2 * members.len() as u32
            },
        },
        members,
    };
    let Some(chat) = ws.chats.get_mut(&lead) else {
        return;
    };
    chat.tasks.clear();
    chat.agents.clear();
    chat.team = None;
    for task in &tasks {
        chat.apply(ThreadEvent::TaskChanged(task.clone()));
    }
    if value != "team-empty" {
        for (number, made) in &owners {
            let task_id = format!("t{number}");
            chat.apply(ThreadEvent::SubagentStarted {
                call_id: team_call_id(&task_id),
                thread_id: member_thread(*number),
                agent: made.role.into(),
                task: made.text.clone(),
                effort: None,
                model: Some("chatgpt/gpt-5.5".into()),
                task_id: Some(task_id),
            });
            let status = match stopped.contains(number) {
                true => TaskStatus::Running,
                false => made.status,
            };
            for event in member_events(*number, status) {
                chat.apply(ThreadEvent::Subagent {
                    thread_id: member_thread(*number),
                    event: Box::new(event),
                });
            }
        }
        for number in &stopped {
            chat.apply(ThreadEvent::Subagent {
                thread_id: member_thread(*number),
                event: Box::new(ThreadEvent::Idle),
            });
        }
        chat.apply(ThreadEvent::Team(team));
    }
    ws.dock.counts = None;
    cx.notify();
}
